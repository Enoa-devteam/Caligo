/** Correlate a single, independently bound field run. The result covers only
 * the sampled closure fields and supplied core journal snapshots. */
import fs from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { crc32 } from 'node:zlib';
import { analyzeCallbackObservationLog } from './analyze-callback-observation.mjs';
import { analyzeTraceLog } from './analyze-text-send-trace.mjs';

const MAX_BYTES = 4 * 1024 * 1024;
const MAX_SEQ = 2_147_483_647;
const FINAL_STATES = new Set(['DeliveryUnknown', 'ConfirmedSuccess', 'ConfirmedFailure', 'AcceptedUnconfirmed']);
const RECORD_TYPES = new Set(['Request', 'Event', 'EventDelivered', 'Gap', 'ReceiptAudit']);
const SUBMIT_STAGES = ['registered', 'submit_enter', 'submit_return'];
const SHUTDOWN_UNKNOWN = 'delivery unknown diagnostic: owner_stopped_with_send_inflight';
const unknown = (reason, details = {}) => ({status: 'unknown', reason, ...details});

function readJournal(journal) {
  const records = [];
  const decoder = new TextDecoder('utf-8', {fatal: true});
  for (let offset = 0; offset < journal.length;) {
    if (journal.length - offset < 16 || journal.readUInt32LE(offset) !== 0x4a474c43 ||
        journal.readUInt16LE(offset + 4) !== 1 || journal.readUInt16LE(offset + 6) !== 0) return null;
    const end = offset + 16 + journal.readUInt32LE(offset + 8);
    if (end > journal.length) return null;
    const body = journal.subarray(offset + 16, end);
    if (crc32(body) !== journal.readUInt32LE(offset + 12)) return null;
    const frame = JSON.parse(decoder.decode(body), (key, value, context) =>
      key === 'payload_hash' && typeof value === 'number' ? BigInt(context.source) : value);
    if (!RECORD_TYPES.has(frame?.t) || !frame.d || typeof frame.d !== 'object' || Array.isArray(frame.d)) return null;
    records.push({offset, type: frame.t, data: frame.d});
    offset = end;
  }
  return records;
}

function requestIdentity(requests) {
  const first = requests[0];
  const last = requests.at(-1);
  const seq = requests[1]?.wire_seq;
  const generation = first?.session_generation;
  const states = requests.map(record => record.state);
  if (typeof first?.request_id !== 'string' || !first.request_id ||
      !Number.isSafeInteger(generation) || generation < 1 ||
      !Number.isInteger(seq) || seq < 1 || seq > MAX_SEQ ||
      states[0] !== 'Queued' || states[1] !== 'DispatchIntent' || states[2] !== 'NativeStarted' ||
      !states.slice(3).every(state => FINAL_STATES.has(state)) || !FINAL_STATES.has(states.at(-1)) ||
      new Set(states).size !== states.length ||
      requests.some((record, i) => record.v !== 1 || record.request_id !== first.request_id ||
        record.session_generation !== generation || (i === 0 ? record.wire_seq !== null : record.wire_seq !== seq))) return null;
  if (last.state === 'ConfirmedSuccess' && (typeof last.native_id !== 'string' || !last.native_id)) return null;
  return {seq, state: last.state};
}

function stagedSuffixMatches(records, requests, beforeLength, {closeAt, shutdownAt}) {
  const start = records.findIndex(record => record.offset === beforeLength);
  if (start < 0 || requests.map(record => record.state).join() !== 'Queued,DispatchIntent,NativeStarted,DeliveryUnknown')
    return false;
  const [queued, intent, started, outcome] = requests;
  const suffix = records.slice(start);
  if (suffix[0]?.type === 'Request' && suffix[0].data === started) suffix.shift();
  if (suffix.length !== 2 || suffix[0].type !== 'ReceiptAudit' ||
      suffix[1].type !== 'Request' || suffix[1].data !== outcome) return false;

  const audit = suffix[0].data;
  const hash = queued.payload_hash;
  const timestamps = [intent.at_unix_ms, started.at_unix_ms, audit.at_unix_ms, outcome.at_unix_ms, shutdownAt];
  return typeof hash === 'bigint' && hash >= 0n && hash <= 0xffff_ffff_ffff_ffffn &&
    requests.every(record => record.payload_hash === hash && record.native_id === null && record.failure_reason === undefined) &&
    audit.v === 1 && audit.request_id === queued.request_id && audit.session_generation === queued.session_generation &&
    audit.detail === SHUTDOWN_UNKNOWN && Number.isSafeInteger(closeAt) && closeAt > 0 && audit.at_unix_ms >= closeAt &&
    timestamps.every((time, index) => Number.isSafeInteger(time) && time > 0 &&
      (index === 0 || time >= timestamps[index - 1]));
}

function stopBoundaries(rows) {
  const orderedPhases = ['armed', 'listener_stopped', 'final_snapshot', 'close_permitted'];
  const positions = rows.flatMap((row, index) => row?.stage === 'stop_observation' ? [index] : []);
  if (positions.length !== orderedPhases.length ||
      positions.some((index, step) => rows[index].ok !== true || rows[index].detail !== orderedPhases[step])) return null;
  const [, listenerStopped, finalSample, closePermitted] = positions;
  const owner = rows[listenerStopped].tid;
  const lastObservation = rows.findLastIndex(row => row?.stage === 'js_callback_observation');
  const shutdown = rows.findIndex(row => row?.stage === 'shutdown' && row.ok === true);
  if (!Number.isInteger(owner) || owner < 1 || rows[finalSample].tid !== owner || rows[closePermitted].tid !== owner ||
      lastObservation < 0 || lastObservation >= finalSample || rows[lastObservation].tid !== owner ||
      shutdown <= closePermitted) return null;
  return {listenerStopped, closeAt: rows[closePermitted].ts, shutdownAt: rows[shutdown].ts};
}

export function analyzeTextStopEvidence(log, finalJournal, beforeStopJournal, {staged = false} = {}) {
  if (typeof log !== 'string' || Buffer.byteLength(log) > MAX_BYTES ||
      !Buffer.isBuffer(finalJournal) || finalJournal.length > MAX_BYTES ||
      (beforeStopJournal !== undefined && (!Buffer.isBuffer(beforeStopJournal) || beforeStopJournal.length > MAX_BYTES)))
    return unknown('invalid_input');
  const callback = analyzeCallbackObservationLog(log);
  if (callback.stop_boundaries.stop_end.status !== 'observed') return unknown('missing_successful_stop');
  if (callback.status !== 'observed') return unknown('missing_associated_callback');

  let request, preStop, records, requests, stop;
  try {
    records = readJournal(finalJournal);
    requests = records?.filter(record => record.type === 'Request').map(record => record.data);
    request = requests && requestIdentity(requests);
    const lines = log.replace(/^\uFEFF/, '').split(/\r?\n/);
    stop = stopBoundaries(lines.map(line => line.trim() ? JSON.parse(line) : null));
    if (stop === null) return unknown('invalid_stop_window');
    preStop = analyzeTraceLog(lines.slice(0, stop.listenerStopped).join('\n'));
  } catch {
    return unknown('invalid_evidence');
  }
  if (!request) return unknown('missing_single_request');
  const trace = analyzeTraceLog(log);
  if (trace.status !== 'observed' || trace.groups.length !== 1 || preStop?.groups?.length !== 1)
    return unknown('missing_single_submit_trace');
  const [submission] = trace.groups;
  const [early] = preStop.groups;
  if (submission.generation !== callback.generation || early.generation !== callback.generation ||
      early.seq !== request.seq || submission.seq !== request.seq ||
      callback.groups.length !== 1 || callback.groups[0].seq !== request.seq) return unknown('request_mismatch');
  const submitStages = group => group.stages.filter(stage => SUBMIT_STAGES.includes(stage)).join();
  if (submitStages(submission) !== SUBMIT_STAGES.join() || submitStages(early) !== SUBMIT_STAGES.join())
    return unknown('submit_count_or_order');

  let journal = 'unverified';
  if (beforeStopJournal !== undefined) {
    if (beforeStopJournal.equals(finalJournal)) journal = 'unchanged';
    else if (staged && finalJournal.subarray(0, beforeStopJournal.length).equals(beforeStopJournal) &&
        stagedSuffixMatches(records, requests, beforeStopJournal.length, stop)) journal = 'staged_close';
    else journal = 'changed';
  }
  const details = {seq: request.seq, listener_generation: callback.generation, journal, request_state: request.state};
  const late = callback.late_callbacks.events;
  if (!late.length) return unknown('missing_late_closure', details);
  if (journal === 'changed' || late.some(event => event.text_state === 'changed'))
    return {status: 'contradicted', reason: 'state_changed', ...details, late_callbacks: late.length};
  if (journal === 'unverified' || late.some(event => event.text_state !== 'unchanged'))
    return unknown('missing_state_comparison', {...details, late_callbacks: late.length});
  return {status: 'corroborated', ...details, late_callbacks: late.length};
}

function readEvidence(path) {
  const fd = fs.openSync(path, 'r');
  try {
    const stat = fs.fstatSync(fd, {bigint: true});
    if (!stat.isFile() || stat.size > BigInt(MAX_BYTES)) throw new Error('invalid input');
    const buffer = Buffer.alloc(Number(stat.size) + 1);
    let size = 0;
    while (size < buffer.length) {
      const count = fs.readSync(fd, buffer, size, buffer.length - size, null);
      if (!count) break;
      size += count;
    }
    if (BigInt(size) !== stat.size) throw new Error('input size changed');
    return {bytes: buffer.subarray(0, size), stat};
  } finally {
    fs.closeSync(fd);
  }
}

function main(args) {
  const staged = args[0] === '--staged';
  const files = staged ? args.slice(1) : args;
  if (files.length !== 3 && (staged || files.length !== 2)) return unknown('usage');
  try {
    const [log, final, before] = files.map(readEvidence);
    if (before && final.stat.dev === before.stat.dev && final.stat.ino === before.stat.ino)
      return unknown('same_journal_snapshot');
    return analyzeTextStopEvidence(log.bytes.toString('utf8'), final.bytes, before?.bytes, {staged});
  } catch {
    return unknown('input_unavailable');
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const result = main(process.argv.slice(2));
  process.stdout.write(JSON.stringify(result) + '\n');
  if (result.status !== 'corroborated') process.exitCode = 2;
}
