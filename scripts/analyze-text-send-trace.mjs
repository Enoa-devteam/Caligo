/** Read-only parser for bounded, sanitized text_send_trace stage logs.
 * Output is observational only: missing reports and native callback threads remain unknown.
 */
import fs from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const MAX_BYTES = 4 * 1024 * 1024;
const MAX_BATCHES = 32;
const MAX_EVENTS = 32;
const MAX_ID = 2_147_483_647;
const STAGES = new Set(['registered','submit_enter','submit_return','promise_accepted',
  'promise_rejected','promise_unknown','matched_add','matched_update',
  'readback_matched','readback_mismatch','settled']);
const unknown = reason => ({status:'unknown', reason, completeness:'unknown'});
const positiveId = value => Number.isSafeInteger(value) && value > 0 && value <= MAX_ID;

/** Parse only the exact whitelisted schema emitted by the current Rust sanitizer. */
export function analyzeTraceLog(input) {
  if (typeof input !== 'string' || Buffer.byteLength(input,'utf8') > MAX_BYTES) return unknown('input_limit');
  const groups = new Map();
  let batches = 0;
  for (const raw of input.replace(/^\uFEFF/, '').split(/\r?\n/)) {
    if (!raw.trim()) continue;
    let row;
    try { row = JSON.parse(raw); } catch { return unknown('invalid_log'); }
    if (!row || typeof row !== 'object' || Array.isArray(row)) return unknown('invalid_log');
    if (row.stage !== 'text_send_trace') continue;
    if (++batches > MAX_BATCHES || row.ok !== true || typeof row.detail !== 'string' ||
        row.detail.length > 8192 || !/^[A-Za-z0-9_'{}\[\]:,]*$/.test(row.detail)) return unknown('invalid_trace');
    let detail;
    const normalized = row.detail.replaceAll("'", '"');
    try { detail = JSON.parse(normalized); } catch { return unknown('invalid_trace'); }
    if (!detail || typeof detail !== 'object' || Array.isArray(detail) ||
        !Number.isSafeInteger(detail.generation) || detail.generation < 0 || detail.generation > MAX_ID ||
        typeof detail.incomplete !== 'boolean' || !Array.isArray(detail.events) || detail.events.length > MAX_EVENTS)
      return unknown('invalid_trace');
    const cleanEvents = [];
    for (const event of detail.events) {
      if (!event || typeof event !== 'object' || Array.isArray(event) || !positiveId(event.seq) || !STAGES.has(event.stage))
        return unknown('invalid_trace');
      cleanEvents.push({seq:event.seq,stage:event.stage});
    }
    // append_stage replaces JSON quotes; canonical reconstruction rejects extra or duplicate keys.
    if (JSON.stringify({events:cleanEvents,generation:detail.generation,incomplete:detail.incomplete}) !== normalized)
      return unknown('invalid_trace');
    if (detail.incomplete || detail.generation === 0) return unknown('incomplete_trace');
    for (const event of cleanEvents) {
      const key = `${detail.generation}:${event.seq}`;
      if (!groups.has(key)) groups.set(key,{generation:detail.generation,seq:event.seq,stages:[]});
      groups.get(key).stages.push(event.stage);
    }
  }
  if (!batches || !groups.size) return unknown('no_trace_records');
  return {status:'observed',batches,completeness:'unknown',scope:'unverified',groups:[...groups.values()]};
}

function main(args) {
  if (args.length !== 1) return unknown('usage');
  let fd;
  try {
    fd = fs.openSync(args[0], 'r');
    if (!fs.fstatSync(fd).isFile()) return unknown('input_unavailable');
    const buffer = Buffer.alloc(MAX_BYTES + 1);
    let size = 0, count;
    while (size < buffer.length && (count = fs.readSync(fd,buffer,size,buffer.length-size,null)) > 0) size += count;
    if (size > MAX_BYTES) return unknown('input_limit');
    return analyzeTraceLog(buffer.subarray(0,size).toString('utf8'));
  } catch { return unknown('input_unavailable'); }
  finally { if (fd !== undefined) { try { fs.closeSync(fd); } catch {} } }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const result = main(process.argv.slice(2));
  process.stdout.write(JSON.stringify(result) + '\n');
  if (result.status !== 'observed') process.exitCode = 2;
}
