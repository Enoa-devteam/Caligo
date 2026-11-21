/** Read-only analysis of sanitized cumulative js_callback_observation reports.
 * This metadata cannot establish log completeness, external request identity or OS TID.
 */
import fs from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const MAX_BYTES = 4 * 1024 * 1024;
const MAX_SNAPSHOTS = 32;
const MAX_EVENTS = 128;
const MAX_ID = 2_147_483_647;
const PHASES = new Set(['active', 'stopping', 'stopped', 'remove_failed']);
const NONE_KINDS = new Set(['recv', 'add', 'update']);
const CLOSURE_KINDS = new Set(['promise_fulfilled', 'promise_rejected', 'readback_fulfilled', 'readback_rejected']);
const STOP_PHASES = {stop_begin: 'stopping', stop_end: 'stopped', stop_remove_failed: 'remove_failed'};
const positiveId = value => Number.isSafeInteger(value) && value > 0 && value <= MAX_ID;
const boundaries = () => Object.fromEntries(Object.keys(STOP_PHASES).map(kind => [kind, {status: 'unknown'}]));
const limits = () => ({completeness: 'unknown', scope: 'unverified', external_request_mapping: 'unverified', os_tid: 'unverified'});
const unknown = reason => ({status: 'unknown', reason, ...limits(), stop_boundaries: boundaries()});

function cleanEvent(event, generation, order) {
  if (!event || typeof event !== 'object' || Array.isArray(event) || event.generation !== generation ||
      event.order !== order || !PHASES.has(event.phase)) return null;
  const {association, current, kind, phase, seq} = event;
  if (association === 'none') {
    if (seq !== 0 || current !== null || !(NONE_KINDS.has(kind) ||
        (Object.hasOwn(STOP_PHASES, kind) && STOP_PHASES[kind] === phase))) return null;
  } else {
    if (!positiveId(seq) || typeof current !== 'boolean') return null;
    if (association === 'matched') {
      if (!['matched_add', 'matched_update'].includes(kind) || phase !== 'active' || current !== true) return null;
    } else if (association !== 'closure' || !CLOSURE_KINDS.has(kind)) return null;
  }
  const hasTid = Object.hasOwn(event, 'callback_tid');
  const tid = event.callback_tid;
  const hasTextState = Object.hasOwn(event, 'text_state');
  const textState = event.text_state;
  if (hasTid && tid !== null && (!Number.isInteger(tid) || tid < 1 || tid > 4_294_967_295)) return null;
  if (hasTextState && (phase !== 'stopped' || association !== 'closure' ||
      !['unchanged', 'changed', 'unverified'].includes(textState))) return null;
  // serde_json's default map ordering, before append_stage substitutes quotes.
  return {association, ...(hasTid ? {callback_tid: tid} : {}), current, generation,
    kind, order, phase, seq, ...(hasTextState ? {text_state: textState} : {})};
}

export function analyzeCallbackObservationLog(input) {
  if (typeof input !== 'string' || Buffer.byteLength(input, 'utf8') > MAX_BYTES) return unknown('input_limit');
  let snapshots = 0, generation;
  let events = [];
  for (const raw of input.replace(/^\uFEFF/, '').split(/\r?\n/)) {
    if (!raw.trim()) continue;
    let row;
    try { row = JSON.parse(raw); } catch { return unknown('invalid_log'); }
    if (!row || typeof row !== 'object' || Array.isArray(row)) return unknown('invalid_log');
    if (row.stage !== 'js_callback_observation') continue;
    if (++snapshots > MAX_SNAPSHOTS) return unknown('snapshot_limit');
    // Reconstruct the envelope too: never accept duplicate or foreign report keys.
    if (Object.keys(row).some(key => !['ts', 'tid', 'stage', 'ok', 'detail'].includes(key)) ||
        JSON.stringify(row) !== raw.trim() || row.ok !== true || typeof row.detail !== 'string' ||
        row.detail.length > 24_576 || !/^[A-Za-z0-9_'{}\[\]:,]*$/.test(row.detail)) return unknown('invalid_snapshot');
    const normalized = row.detail.replaceAll("'", '"');
    let detail;
    try { detail = JSON.parse(normalized); } catch { return unknown('invalid_snapshot'); }
    if (!detail || typeof detail !== 'object' || Array.isArray(detail) || !positiveId(detail.generation) ||
        typeof detail.incomplete !== 'boolean' || !Array.isArray(detail.events) ||
        detail.events.length > MAX_EVENTS) return unknown('invalid_snapshot');
    const clean = detail.events.map((event, index) => cleanEvent(event, detail.generation, index + 1));
    if (clean.some(event => event === null) ||
        JSON.stringify({events: clean, generation: detail.generation, incomplete: detail.incomplete}) !== normalized)
      return unknown('invalid_snapshot');
    if (detail.incomplete) return unknown('incomplete_snapshot');
    if (generation !== undefined && generation !== detail.generation) return unknown('multiple_generations');
    generation = detail.generation;
    if (clean.length < events.length || events.some((event, index) => JSON.stringify(event) !== JSON.stringify(clean[index])))
      return unknown('inconsistent_prefix');
    events = clean;
  }
  const stop_boundaries = boundaries();
  let phase = 'active', terminalBoundary;
  for (const event of events) {
    if (event.kind === 'stop_begin') {
      if (phase !== 'active') return unknown('invalid_phase_order');
      phase = 'stopping';
    } else if (event.kind === 'stop_end' || event.kind === 'stop_remove_failed') {
      if (phase !== 'stopping') return unknown('invalid_phase_order');
      phase = STOP_PHASES[event.kind];
      terminalBoundary = event;
    }
    if (event.phase !== phase) return unknown('invalid_phase_order');
    if (Object.hasOwn(STOP_PHASES, event.kind)) {
      stop_boundaries[event.kind] = {status: 'observed', orders: [event.order]};
    }
  }
  if (!events.some(event => !Object.hasOwn(STOP_PHASES, event.kind))) {
    return {...unknown('no_callbacks'), stop_boundaries};
  }
  const afterStop = terminalBoundary ? events.filter(event => event.order > terminalBoundary.order) : [];
  const late = afterStop.filter(event => event.association === 'closure' && event.seq > 0);
  const late_callbacks = {
    status: late.length ? 'observed' : 'unknown',
    boundary: terminalBoundary?.kind ?? 'unobserved',
    unassociated_count: afterStop.filter(event => event.association === 'none').length,
    state_isolation: 'unverified',
    events: late.map(event => {
      const result = {generation, seq:event.seq, order:event.order, kind:event.kind,
        phase:event.phase, association:event.association, current:event.current};
      if (Object.hasOwn(event, 'callback_tid')) result.callback_tid = event.callback_tid;
      if (Object.hasOwn(event, 'text_state')) result.text_state = event.text_state;
      return result;
    }),
  };
  const groups = new Map();
  const unassociated = [];
  for (const event of events) {
    const {order, kind, phase, association, current, seq} = event;
    if (Object.hasOwn(STOP_PHASES, kind)) continue;
    if (seq === 0) {
      unassociated.push({order, kind, phase});
    } else {
      if (!groups.has(seq)) groups.set(seq, {generation, seq, kinds: [], phases: [], events: []});
      const group = groups.get(seq);
      if (!group.kinds.includes(kind)) group.kinds.push(kind);
      if (!group.phases.includes(phase)) group.phases.push(phase);
      const observed = {order, kind, phase, association, current};
      if (Object.hasOwn(event, 'callback_tid')) observed.callback_tid = event.callback_tid;
      if (Object.hasOwn(event, 'text_state')) observed.text_state = event.text_state;
      group.events.push(observed);
    }
  }
  return {status: 'observed', snapshots, generation, event_count: events.length, ...limits(),
    groups: [...groups.values()], unassociated, stop_boundaries, late_callbacks};
}

function main(args) {
  if (args.length !== 1) return unknown('usage');
  let fd;
  try {
    fd = fs.openSync(args[0], 'r');
    if (!fs.fstatSync(fd).isFile()) return unknown('input_unavailable');
    const buffer = Buffer.alloc(MAX_BYTES + 1);
    let size = 0, count;
    while (size < buffer.length && (count = fs.readSync(fd, buffer, size, buffer.length - size, null)) > 0) size += count;
    if (size > MAX_BYTES) return unknown('input_limit');
    return analyzeCallbackObservationLog(buffer.subarray(0, size).toString('utf8'));
  } catch { return unknown('input_unavailable'); }
  finally { if (fd !== undefined) { try { fs.closeSync(fd); } catch {} } }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const result = main(process.argv.slice(2));
  process.stdout.write(JSON.stringify(result) + '\n');
  if (result.status !== 'observed') process.exitCode = 2;
}
