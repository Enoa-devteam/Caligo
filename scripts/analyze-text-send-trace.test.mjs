import test from 'node:test';
import assert from 'node:assert/strict';
import { analyzeTraceLog } from './analyze-text-send-trace.mjs';

const detail = (generation, incomplete, events) => JSON.stringify({events,generation,incomplete}).replaceAll('"', "'");
const line = (stage, value, ok=true) => JSON.stringify({stage,ok,detail:value});

test('correlates fixed labels across batches without exposing other log content', () => {
  const log=[line('listener_armed','secret-account'),
    line('text_send_trace',detail(1,false,[{seq:1,stage:'registered'},{seq:2,stage:'registered'}])),
    line('text_send_trace',detail(1,false,[{seq:2,stage:'promise_accepted'},{seq:1,stage:'promise_unknown'}]))].join('\n');
  const result=analyzeTraceLog(log);
  assert.deepEqual(result,{status:'observed',batches:2,completeness:'unknown',scope:'unverified',groups:[
    {generation:1,seq:1,stages:['registered','promise_unknown']},
    {generation:1,seq:2,stages:['registered','promise_accepted']}]});
  assert.ok(!JSON.stringify(result).includes('secret-account'));
});

test('treats missing, incomplete, malformed or noncanonical data as unknown', () => {
  assert.equal(analyzeTraceLog(line('other','secret')).status,'unknown');
  for (const bad of [line('text_send_trace',detail(1,true,[])),line('text_send_trace',"{'events':[],'generation':1,'incomplete':false,'secret':'x'}"),
    line('text_send_trace',detail(1,false,[{seq:1,stage:'secret'}])),line('text_send_trace',"{'events':[],'generation':1,'generation':2,'incomplete':false}"),line('text_send_trace',detail(1,false,[{seq:0,stage:'registered'}])),
    line('text_send_trace',detail(1,false,[{seq:1,stage:'registered'}]),false),'not-json']) {
    const result=analyzeTraceLog(bad);assert.equal(result.status,'unknown');assert.ok(!JSON.stringify(result).includes('secret'));
  }
});

test('rejects over-budget logs and events', () => {
  assert.equal(analyzeTraceLog('x'.repeat(4*1024*1024+1)).status,'unknown');
  const events=Array.from({length:33},()=>({seq:1,stage:'registered'}));
  assert.equal(analyzeTraceLog(line('text_send_trace',detail(1,false,events))).status,'unknown');
  const rows=Array.from({length:33},()=>line('text_send_trace',detail(1,false,[]))).join('\n');
  assert.equal(analyzeTraceLog(rows).status,'unknown');
});
