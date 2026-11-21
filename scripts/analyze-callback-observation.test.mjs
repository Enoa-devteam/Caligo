import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { analyzeCallbackObservationLog as analyze } from './analyze-callback-observation.mjs';

const event = (order, seq = 1, kind = 'promise_fulfilled', phase = 'active', association = 'closure', current = true, generation = 1) =>
  ({association, current, generation, kind, order, phase, seq});
const detail = (events, generation = 1, incomplete = false) => JSON.stringify({events, generation, incomplete}).replaceAll('"', "'");
const line = (value, ok = true) => JSON.stringify({ts: 123, tid: 999, stage: 'js_callback_observation', ok, detail: value});
const snapshot = (events, generation = 1, incomplete = false) => line(detail(events, generation, incomplete));
const stop = (order, kind, phase) => event(order, 0, kind, phase, 'none', null);
const isUnknown = input => {
  const result = analyze(input);
  assert.equal(result.status, 'unknown');
  assert.equal(result.completeness, 'unknown');
  assert.equal(result.external_request_mapping, 'unverified');
  assert.equal(result.os_tid, 'unverified');
  return result;
};

test('stop-observation reason rows do not change callback analysis', () => {
  const events = [event(1), event(2, 1, 'matched_add', 'active', 'matched')];
  const base = snapshot(events);
  const withReason = [
    JSON.stringify({ts: 9, tid: 1, stage: 'stop_observation_reason', ok: false, detail: 'observation_incomplete'}),
    base,
    JSON.stringify({ts: 10, tid: 1, stage: 'stop_observation', ok: false, detail: 'window_failed; roots retained'}),
  ].join('\n');
  assert.deepEqual(analyze(withReason), analyze(base));
});

test('reversed request completion order groups by seq without joining report TIDs', () => {
  const events = [event(1, 2), event(2, 1, 'readback_rejected', 'active', 'closure', false)];
  const result = analyze([JSON.stringify({stage: 'other', detail: 'PRIVATE_ACCOUNT'}), snapshot(events)].join('\n'));
  assert.equal(result.status, 'observed');
  assert.deepEqual(result.groups.map(group => [group.seq, group.kinds, group.phases]),
    [[2, ['promise_fulfilled'], ['active']], [1, ['readback_rejected'], ['active']]]);
  assert.equal(result.groups[1].events[0].current, false);
  assert.equal(result.completeness, 'unknown');
  assert.equal(result.external_request_mapping, 'unverified');
  assert.equal(result.os_tid, 'unverified');
  assert.ok(!JSON.stringify(result).includes('PRIVATE_ACCOUNT'));
  assert.ok(!JSON.stringify(result).includes('999'));
});

test('cumulative and identical snapshots do not duplicate events', () => {
  const first = [event(1)];
  const full = [...first, event(2, 1, 'matched_add', 'active', 'matched')];
  const result = analyze([snapshot([]), snapshot(first), snapshot(first), snapshot(full)].join('\n'));
  assert.equal(result.status, 'observed');
  assert.equal(result.snapshots, 4);
  assert.equal(result.event_count, 2);
  assert.deepEqual(result.groups[0].events.map(value => value.order), [1, 2]);
});

test('prefix rollback, alteration and listener generation changes are unknown', () => {
  const full = [event(1), event(2)];
  isUnknown([snapshot(full), snapshot([event(1)])].join('\n'));
  isUnknown([snapshot([event(1)]), snapshot([event(1, 2), event(2)])].join('\n'));
  isUnknown([snapshot(full), snapshot([])].join('\n'));
  isUnknown([snapshot([event(1)]), snapshot([event(1, 1, 'promise_fulfilled', 'active', 'closure', true, 2)], 2)].join('\n'));
});

test('missing stop boundaries stay unknown while callbacks are observed', () => {
  const result = analyze(snapshot([event(1)]));
  assert.equal(result.status, 'observed');
  assert.deepEqual(result.stop_boundaries, {stop_begin: {status: 'unknown'}, stop_end: {status: 'unknown'}, stop_remove_failed: {status: 'unknown'}});
});

test('stop boundaries and callbacks during/after stop retain their actual metadata', () => {
  const result = analyze(snapshot([event(1), stop(2, 'stop_begin', 'stopping'),
    event(3, 1, 'promise_rejected', 'stopping'),
    stop(4, 'stop_end', 'stopped'), event(5, 1, 'readback_fulfilled', 'stopped')]));
  assert.equal(result.status, 'observed');
  assert.deepEqual(result.stop_boundaries, {stop_begin: {status: 'observed', orders: [2]},
    stop_end: {status: 'observed', orders: [4]}, stop_remove_failed: {status: 'unknown'}});
  assert.deepEqual(result.groups[0].phases, ['active', 'stopping', 'stopped']);
});

test('unassociated listener callbacks are observed without manufacturing request groups', () => {
  const result = analyze(snapshot([event(1, 0, 'recv', 'active', 'none', null)]));
  assert.equal(result.status, 'observed');
  assert.deepEqual(result.groups, []);
  assert.deepEqual(result.unassociated, [{order: 1, kind: 'recv', phase: 'active'}]);
});

test('no reports, empty snapshots and stop markers alone do not prove callbacks', () => {
  for (const input of ['', JSON.stringify({stage: 'other'}), snapshot([]),
    snapshot([stop(1, 'stop_begin', 'stopping'), stop(2, 'stop_end', 'stopped')])]) isUnknown(input);
});

test('a marker-only window preserves stop boundaries and leaves callback status unknown', () => {
  const begin = stop(1, 'stop_begin', 'stopping');
  const end = stop(2, 'stop_end', 'stopped');
  const result = isUnknown([snapshot([]), snapshot([begin]), snapshot([begin, end])].join('\n'));
  assert.equal(result.reason, 'no_callbacks');
  assert.deepEqual(result.stop_boundaries, {
    stop_begin: {status: 'observed', orders: [1]},
    stop_end: {status: 'observed', orders: [2]},
    stop_remove_failed: {status: 'unknown'},
  });
  assert.equal(isUnknown(snapshot([begin])).stop_boundaries.stop_end.status, 'unknown');
});

test('a marker-only removal failure preserves the failure boundary', () => {
  const result = isUnknown(snapshot([stop(1, 'stop_begin', 'stopping'), stop(2, 'stop_remove_failed', 'remove_failed')]));
  assert.equal(result.reason, 'no_callbacks');
  assert.deepEqual(result.stop_boundaries.stop_remove_failed, {status: 'observed', orders: [2]});
  assert.equal(result.stop_boundaries.stop_end.status, 'unknown');
});

test('contradictory marker-only windows expose no verified boundaries', () => {
  const traces = [
    [stop(1, 'stop_end', 'stopped')],
    [stop(1, 'stop_begin', 'stopping'), stop(2, 'stop_begin', 'stopping')],
    [stop(1, 'stop_begin', 'stopping'), stop(2, 'stop_remove_failed', 'remove_failed'), stop(3, 'stop_end', 'stopped')],
  ];
  for (const events of traces) {
    const result = isUnknown(snapshot(events));
    assert.equal(result.reason, 'invalid_phase_order');
    assert.ok(Object.values(result.stop_boundaries).every(boundary => boundary.status === 'unknown'));
  }
});

test('incomplete, failed and report budget exhaustion invalidate the observation', () => {
  const good = snapshot([event(1)]);
  for (const bad of [snapshot([event(1)], 1, true), line(detail([event(1)]), false),
    line("{'incomplete':true,'reason':'report_budget_exhausted'}", false),
    line("{'incomplete':true,'reason':'report_budget_exhausted'}")]) isUnknown(`${good}\n${bad}`);
});

test('rejects order gaps, duplicate/reversed orders and invalid generations', () => {
  for (const events of [[event(2)], [event(1), event(3)], [event(1), event(1)], [event(2), event(1)]]) isUnknown(snapshot(events));
  for (const generation of [0, -1, 1.5, 2147483648, '1', null]) isUnknown(snapshot([], generation));
  isUnknown(snapshot([event(1, 1, 'promise_fulfilled', 'active', 'closure', true, 2)]));
});

test('requires none/zero/null or compatible positive seq/boolean associations', () => {
  const invalid = [event(1, 0), event(1, 2147483648), event(1, 1.5), event(1, '1'),
    event(1, 1, 'recv', 'active', 'none', null), event(1, 0, 'recv', 'active', 'none', false),
    event(1, 1, 'promise_fulfilled', 'active', 'closure', null),
    event(1, 1, 'matched_add', 'active', 'closure'), event(1, 1, 'promise_rejected', 'active', 'matched'),
    event(1, 1, 'matched_update', 'stopped', 'matched'), event(1, 1, 'matched_add', 'active', 'matched', false),
    event(1, 0, 'stop_end', 'active', 'none', null), event(1, 1, 'promise_fulfilled', 'foreign'),
    event(1, 1, 'promise_fulfilled', 'active', 'foreign')];
  for (const value of invalid) isUnknown(snapshot([value]));
  assert.equal(analyze(snapshot([event(1, 2147483647)])).status, 'observed');
});

test('canonical reconstruction rejects extra and duplicate detail, event and envelope keys', () => {
  const valid = detail([event(1)]);
  for (const value of [valid.replace("'incomplete':false", "'incomplete':false,'private':'SECRET_SENTINEL'"),
    valid.replace("'generation':1,'incomplete'", "'generation':1,'generation':1,'incomplete'"),
    valid.replace("'seq':1", "'seq':1,'seq':1"), valid.replace("'seq':1", "'seq':1,'private':'SECRET_SENTINEL'"),
    valid.replace("'current':true", "'current':1"), valid.replace("'order':1", "'order':1.0")]) {
    const result = isUnknown(line(value));
    assert.ok(!JSON.stringify(result).includes('SECRET_SENTINEL'));
  }
  isUnknown(line(valid).replace('"ok":true', '"ok":true,"ok":true'));
  isUnknown(line(valid).replace('"ts":123', '"ts":123,"private":"SECRET_SENTINEL"'));
});

test('malicious and malformed inputs never echo secrets or parser errors', () => {
  for (const input of ['SECRET_SENTINEL', 'null', '[]', line("{'events':SECRET_SENTINEL}"),
    line("{'events':[],'generation':1,'incomplete':false,'__proto__':{'private':'SECRET_SENTINEL'}}"),
    line("SECRET_SENTINEL\\n\\u0027"), snapshot([{...event(1), kind: 'SECRET_SENTINEL'}])]) {
    assert.ok(!JSON.stringify(isUnknown(input)).includes('SECRET_SENTINEL'));
  }
});

test('enforces byte, snapshot and event bounds including accepted endpoints', () => {
  isUnknown('é'.repeat(2 * 1024 * 1024 + 1));
  isUnknown(snapshot(Array.from({length: 129}, (_, index) => event(index + 1))));
  const full = snapshot(Array.from({length: 128}, (_, index) => event(index + 1)));
  assert.equal(analyze(full).event_count, 128);
  assert.equal(analyze(Array(32).fill(full).join('\n')).status, 'observed');
  isUnknown(Array(33).fill(full).join('\n'));
  const base = snapshot([event(1)]);
  assert.equal(analyze(base + ' '.repeat(4 * 1024 * 1024 - Buffer.byteLength(base))).status, 'observed');
  assert.equal(analyze('\uFEFF' + base + '\r\n').status, 'observed');
});

test('optional synchronous callback TID is preserved only from its event, never envelope', () => {
  const withTid = tid => ({association:'closure',callback_tid:tid,current:true,generation:1,
    kind:'promise_fulfilled',order:1,phase:'active',seq:1});
  for (const tid of [1, 4_294_967_295, null]) {
    const result=analyze(snapshot([withTid(tid)]));
    assert.equal(result.status,'observed');
    assert.equal(result.groups[0].events[0].callback_tid,tid);
    assert.equal(result.os_tid,'unverified');
  }
  for(const tid of [0,-1,4_294_967_296,1.5,'999',{},true]) isUnknown(snapshot([withTid(tid)]));
  assert.ok(!Object.hasOwn(analyze(snapshot([event(1)])).groups[0].events[0],'callback_tid'));
});

test('stop ordering rejects contradictory, duplicate and unanchored phases', () => {
  const traces = [
    [event(1), stop(2,'stop_end','stopped')],
    [event(1), stop(2,'stop_begin','stopping'), stop(3,'stop_begin','stopping')],
    [event(1), stop(2,'stop_begin','stopping'), stop(3,'stop_remove_failed','remove_failed'), stop(4,'stop_end','stopped')],
    [event(1), stop(2,'stop_begin','stopping'), event(3)],
    [event(1), event(2,1,'promise_rejected','stopping')],
    [event(1), stop(2,'stop_begin','stopping'), stop(3,'stop_end','stopped'), event(4)],
  ];
  for (const events of traces) assert.equal(isUnknown(snapshot(events)).reason, 'invalid_phase_order');
});

test('late summary requires a completed stop boundary and direct request association', () => {
  const start = [event(1),stop(2,'stop_begin','stopping')];
  const during = analyze(snapshot([...start,event(3,1,'promise_rejected','stopping')]));
  assert.equal(during.status,'observed');
  assert.equal(during.late_callbacks.status,'unknown');
  const unassociated = analyze(snapshot([...start,stop(3,'stop_end','stopped'),event(4,0,'recv','stopped','none',null)]));
  assert.equal(unassociated.late_callbacks.status,'unknown');
  assert.equal(unassociated.late_callbacks.unassociated_count,1);
  const associated = analyze(snapshot([...start,stop(3,'stop_end','stopped'),
    {association:'closure',callback_tid:77,current:true,generation:1,kind:'readback_fulfilled',order:4,phase:'stopped',seq:1}]));
  assert.equal(associated.late_callbacks.status,'observed');
  assert.deepEqual(associated.late_callbacks.events,[{generation:1,seq:1,order:4,kind:'readback_fulfilled',
    phase:'stopped',association:'closure',current:true,callback_tid:77}]);
  assert.equal(associated.late_callbacks.state_isolation,'unverified');
  const failed = analyze(snapshot([...start,stop(3,'stop_remove_failed','remove_failed'),event(4,1,'promise_rejected','remove_failed')]));
  assert.equal(failed.late_callbacks.status,'observed');
  assert.equal(failed.late_callbacks.boundary,'stop_remove_failed');
});

test('stopped closure preserves bounded text comparison without certifying wider isolation', () => {
  const before = [stop(1, 'stop_begin', 'stopping'), stop(2, 'stop_end', 'stopped')];
  for (const text_state of ['unchanged', 'changed', 'unverified']) {
    const late = {...event(3, 7, 'readback_rejected', 'stopped'), text_state};
    const result = analyze(snapshot([...before, late]));
    assert.equal(result.late_callbacks.status, 'observed');
    assert.equal(result.late_callbacks.events[0].text_state, text_state);
    assert.equal(result.late_callbacks.state_isolation, 'unverified');
  }
  for (const late of [
    {...event(3, 7, 'readback_rejected', 'stopped'), text_state: 'PRIVATE_SENTINEL'},
    {...event(3, 0, 'recv', 'stopped', 'none', null), text_state: 'unchanged'},
    {...event(1), text_state: 'unchanged'},
  ]) {
    const input = snapshot(late.order === 1 ? [late] : [...before, late]);
    assert.ok(!JSON.stringify(isUnknown(input)).includes('PRIVATE_SENTINEL'));
  }
});

test('CLI reads files without mutation, emits safe JSON, and exits 2 for unknown', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'caligo-callback-analyzer-'));
  const file = path.join(directory, 'synthetic.jsonl');
  const script = fileURLToPath(new URL('./analyze-callback-observation.mjs', import.meta.url));
  const run = args => spawnSync(process.execPath, [script, ...args], {encoding: 'utf8', timeout: 10_000});
  try {
    const input = snapshot([event(1)]);
    fs.writeFileSync(file, input);
    const observed = run([file]);
    assert.equal(observed.status, 0);
    assert.equal(JSON.parse(observed.stdout).status, 'observed');
    assert.equal(fs.readFileSync(file, 'utf8'), input);
    fs.writeFileSync(file, 'SECRET_SENTINEL');
    for (const args of [[file], [], [directory], [path.join(directory, 'SECRET_SENTINEL')]]) {
      const result = run(args);
      assert.equal(result.status, 2);
      assert.equal(JSON.parse(result.stdout).status, 'unknown');
      assert.ok(!(result.stdout + result.stderr).includes('SECRET_SENTINEL'));
    }
    fs.writeFileSync(file, Buffer.alloc(4 * 1024 * 1024 + 1));
    assert.equal(JSON.parse(run([file]).stdout).reason, 'input_limit');
  } finally { fs.rmSync(directory, {recursive: true, force: true}); }
});
