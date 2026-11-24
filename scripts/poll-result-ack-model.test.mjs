import test from 'node:test';
import assert from 'node:assert/strict';
import { PollResultAckModel } from './poll-result-ack-model.mjs';
const model = (options = {}) => new PollResultAckModel({generation:'listener-1', ...options});

test('unacknowledged batch repeats; acceptance then acknowledgement advances', () => {
  const m=model({maxBatch:1}); m.put('a',{status:'done',msgId:'11'});m.put('b',{status:'unknown'});
  const first=m.poll(); first.results[0].value.msgId='mutated';
  assert.equal(m.poll().results[0].value.msgId,'11');
  assert.equal(m.size,2);assert.ok(m.acknowledge(first.token));assert.equal(m.size,1);
  assert.equal(m.poll().results[0].key,'b');assert.equal(m.acknowledge(first.token),false);
});

test('outdated acknowledgement does not delete a newer revision', () => {
  const m=model();m.put('a',{status:'unknown',reason:'timeout'});m.put('b',{status:'done',msgId:'12'});
  const batch=m.poll();m.put('a',{status:'done',msgId:'11'});assert.ok(m.acknowledge(batch.token));
  assert.deepEqual(m.inspect('a'),{status:'done',msgId:'11'});assert.equal(m.inspect('b'),undefined);
  assert.equal(m.poll().results[0].value.msgId,'11');
});

test('wrong generation, forged token and duplicate acknowledgement do not clear results', () => {
  const a=model(),b=model({generation:'listener-2'});a.put('k',{status:'done'});b.put('k',{status:'unknown'});
  const x=a.poll(),y=b.poll();assert.equal(b.acknowledge(x.token),false);
  assert.equal(b.acknowledge({generation:y.token.generation,batch:y.token.batch+1}),false);
  assert.equal(b.size,1);assert.ok(b.acknowledge(y.token));assert.equal(b.acknowledge(y.token),false);
});

test('capacity, oversized input and invalid input fail closed without eviction', () => {
  const m=model({capacity:2,maxBytes:80});assert.ok(m.put('a',{status:'unknown'}));assert.ok(m.put('b',{status:'pending'}));
  assert.equal(m.put('c',{status:'done'}),false);assert.equal(m.put('a',{status:'done',msgId:'x'.repeat(200)}),false);
  assert.equal(m.put('a',{status:'other'}),false);assert.equal(m.size,2);assert.equal(m.inspect('a').status,'unknown');
  assert.ok(m.put('b',{status:'done',msgId:'12'}));assert.equal(m.size,2);
});

test('nonterminal states are never acknowledged as terminal results', () => {
  const m=model();m.put('a',{status:'accepted'});m.put('b',{status:'pending'});assert.equal(m.poll(),null);
  m.put('a',{status:'done',msgId:'12'});const batch=m.poll();assert.deepEqual(batch.results.map(x=>x.key),['a']);
  assert.ok(m.acknowledge(batch.token));assert.equal(m.inspect('b').status,'pending');
});

test('revision and batch exhaustion reject new work and never wrap', () => {
  const m=model({sequenceLimit:2});assert.ok(m.put('a',{status:'done'}));let x=m.poll();assert.ok(m.acknowledge(x.token));
  assert.ok(m.put('b',{status:'done'}));x=m.poll();assert.ok(m.acknowledge(x.token));
  assert.equal(m.put('c',{status:'done'}),false);assert.equal(m.size,0);
});
