import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { crc32 } from 'node:zlib';
import { analyzeTextStopEvidence as analyze } from './analyze-text-stop-evidence.mjs';

const seq = 47;
const generation = 11;
const requestId = 'PRIVATE_FIXTURE_REQUEST_ID';
const line = (stage, detail) => JSON.stringify({ts: 1000, tid: 23, stage, ok: true, detail});
const batch = (stage, events) => line(stage, JSON.stringify({events, generation, incomplete: false}).replaceAll('"', "'"));
const trace = (stages, requestSeq = seq) => batch('text_send_trace', stages.map(stage => ({seq: requestSeq, stage})));
const submit = ['registered', 'submit_enter', 'submit_return'];
const marker = (order, kind, phase) =>
  ({association: 'none', current: null, generation, kind, order, phase, seq: 0});
const closure = (order, phase, textState) => ({association: 'closure', current: false, generation,
  kind: 'promise_fulfilled', order, phase, seq, ...(textState === undefined ? {} : {text_state: textState})});

function report({textState = 'unchanged', stoppedClosure = true, traceRows = [trace(submit)], afterFinal = false} = {}) {
  const begin = marker(1, 'stop_begin', 'stopping');
  const end = marker(2, 'stop_end', 'stopped');
  const events = stoppedClosure ? [begin, end, closure(3, 'stopped', textState)] :
    [closure(1, 'active'), {...begin, order: 2}, {...end, order: 3}];
  const observation = batch('js_callback_observation', events);
  return [...traceRows, line('stop_observation', 'armed'), line('stop_observation', 'listener_stopped'),
    ...(!afterFinal ? [observation] : []), line('stop_observation', 'final_snapshot'),
    line('stop_observation', 'close_permitted'), line('shutdown', 'closed'),
    ...(afterFinal ? [observation] : [])].join('\n');
}

function frame(type, data) {
  const body = Buffer.from(JSON.stringify({t: type, d: data}));
  const header = Buffer.alloc(16);
  header.writeUInt32LE(0x4a474c43, 0);
  header.writeUInt16LE(1, 4);
  header.writeUInt32LE(body.length, 8);
  header.writeUInt32LE(crc32(body), 12);
  return Buffer.concat([header, body]);
}

function journal(requestSeq = seq, finalState = 'DeliveryUnknown', nativeId = 'PRIVATE_FIXTURE_NATIVE_ID') {
  return Buffer.concat(['Queued', 'DispatchIntent', 'NativeStarted', finalState].map((state, index) =>
    frame('Request', {v: 1, request_id: requestId, session_generation: 3,
      state, native_id: state === 'ConfirmedSuccess' ? nativeId : null,
      wire_seq: index === 0 ? null : requestSeq})));
}

function stagedJournals({startedBefore = false, reason, at = 1000, finalHash = '18446744073709551614', extra} = {}) {
  const request = (state, hash = '18446744073709551614') => frame('Request', {
    v: 1, request_id: requestId, session_generation: 3, state, native_id: null,
    wire_seq: state === 'Queued' ? null : seq, payload_hash: JSON.rawJSON(hash),
    at_unix_ms: state === 'DeliveryUnknown' ? at : 400,
  });
  const started = request('NativeStarted');
  const before = Buffer.concat([request('Queued'), request('DispatchIntent'), ...(startedBefore ? [started] : [])]);
  const audit = frame('ReceiptAudit', {
    v: 1, request_id: requestId, session_generation: 3,
    detail: reason ?? 'delivery unknown diagnostic: owner_stopped_with_send_inflight', at_unix_ms: at,
  });
  const after = Buffer.concat([before, ...(!startedBefore ? [started] : []),
    audit, request('DeliveryUnknown', finalHash), ...(extra ? [extra] : [])]);
  return [after, before];
}

test('staged mode admits only the bound owner shutdown suffix', () => {
  for (const startedBefore of [false, true]) {
    const pair = stagedJournals({startedBefore});
    const measured = analyze(report(), ...pair, {staged: true});
    assert.equal(measured.status, 'corroborated');
    assert.equal(measured.journal, 'staged_close');
    assert.equal(measured.request_state, 'DeliveryUnknown');
    assert.equal(analyze(report(), ...pair).status, 'contradicted');
    assert.ok(!JSON.stringify(measured).includes(requestId));
  }
});

test('a staged journal suffix does not substitute for an unchanged late callback', () => {
  const pair = stagedJournals();
  const missing = analyze(report({stoppedClosure: false}), ...pair, {staged: true});
  assert.equal(missing.reason, 'missing_late_closure');
  assert.equal(missing.journal, 'staged_close');
  assert.equal(analyze(report({textState: 'changed'}), ...pair, {staged: true}).status, 'contradicted');
});

test('different cause, timing, exact payload hash or unsolicited frames reject the staged suffix', () => {
  for (const options of [
    {reason: 'delivery unknown diagnostic: wrong cause'},
    {at: 999},
    {at: 1001},
    {finalHash: '18446744073709551615'},
    {extra: frame('EventDelivered', {event_seq: 1})},
  ]) {
    assert.equal(analyze(report(), ...stagedJournals(options), {staged: true}).status, 'contradicted');
  }
  const [after, before] = stagedJournals();
  assert.equal(analyze(report(), after, before.subarray(0, before.length - 1), {staged: true}).status, 'contradicted');
});

test('same sequence across all three sources corroborates the sampled fields', () => {
  const captured = journal();
  const result = analyze(report(), captured, Buffer.from(captured));
  assert.deepEqual(result, {status: 'corroborated', seq, listener_generation: generation,
    journal: 'unchanged', request_state: 'DeliveryUnknown', late_callbacks: 1});
  assert.ok(!JSON.stringify(result).includes('PRIVATE_FIXTURE'));
});

test('an early closure leaves isolation unknown even when the sources match', () => {
  const captured = journal(seq, 'ConfirmedSuccess');
  const result = analyze(report({stoppedClosure: false}), captured);
  assert.equal(result.reason, 'missing_late_closure');
  assert.equal(result.seq, seq);
  assert.equal(result.journal, 'unverified');
});

test('changed callback fields or journal bytes contradict the comparison', () => {
  const captured = journal();
  assert.equal(analyze(report({textState: 'changed'}), captured, captured).status, 'contradicted');
  assert.equal(analyze(report(), captured, journal(seq, 'ConfirmedSuccess')).journal, 'changed');
  assert.equal(analyze(report(), captured, journal(seq, 'ConfirmedSuccess')).status, 'contradicted');
});

test('a missing comparison or an unverified callback remains unknown', () => {
  const captured = journal();
  assert.equal(analyze(report(), captured).reason, 'missing_state_comparison');
  assert.equal(analyze(report({textState: 'unverified'}), captured, captured).reason, 'missing_state_comparison');
});

test('trace and journal must match the callback sequence independently', () => {
  assert.equal(analyze(report(), journal(seq + 1)).reason, 'request_mismatch');
  assert.equal(analyze(report({traceRows: [trace(submit, seq + 1)]}), journal()).reason, 'request_mismatch');
});

test('duplicated submissions and missing prefix stages cannot give positive evidence', () => {
  assert.equal(analyze(report({traceRows: [trace(submit), trace(submit)]}), journal()).reason, 'submit_count_or_order');
  assert.equal(analyze(report({traceRows: [trace(submit.slice(1))]}), journal()).reason, 'submit_count_or_order');
});

test('post-close observations and broken stop boundaries are rejected', () => {
  assert.equal(analyze(report({afterFinal: true}), journal()).reason, 'invalid_stop_window');
  assert.equal(analyze(report().replace('close_permitted', 'other'), journal()).reason, 'invalid_stop_window');
});

test('damaged and multiple-request journals leave the mapping unknown', () => {
  const original = journal();
  assert.equal(analyze(report(), original.subarray(0, original.length - 1)).reason, 'missing_single_request');
  const corrupted = Buffer.from(original);
  corrupted[20] ^= 1;
  assert.equal(analyze(report(), corrupted).reason, 'missing_single_request');
  const second = frame('Request', {v: 1, request_id: 'OTHER', state: 'Queued', wire_seq: null, session_generation: 3});
  assert.equal(analyze(report(), Buffer.concat([original, second])).reason, 'missing_single_request');
  assert.equal(analyze(report(), journal(seq, 'ConfirmedSuccess', null)).reason, 'missing_single_request');
});

test('CLI refuses to use one journal file as both stopping snapshots', t => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'caligo-text-stop-'));
  const logPath = path.join(tmp, 'entry.jsonl');
  const journalPath = path.join(tmp, 'journal');
  t.after(() => {
    fs.rmSync(logPath, {force: true});
    fs.rmSync(journalPath, {force: true});
    fs.rmdirSync(tmp);
  });
  fs.writeFileSync(logPath, report());
  fs.writeFileSync(journalPath, journal());
  const script = fileURLToPath(new URL('./analyze-text-stop-evidence.mjs', import.meta.url));
  const result = spawnSync(process.execPath, [script, logPath, journalPath, journalPath], {encoding: 'utf8'});
  assert.equal(result.status, 2);
  assert.equal(JSON.parse(result.stdout).reason, 'same_journal_snapshot');
});
