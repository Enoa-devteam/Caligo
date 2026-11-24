import test from 'node:test';
import assert from 'node:assert/strict';
import { LabSessionModel } from './lbr-session-model.mjs';

const id='1'.repeat(32), handle='2'.repeat(32), other='3'.repeat(32);
const ready={isolated:true,privateOutput:true,bounded:true,noConflict:true,clockDefined:true};
const opened=() => { const m=new LabSessionModel(id); m.preflight(ready); m.begin(); m.started({kind:'started',identity:id,handle}); return m; };

test('successful lifecycle stops only the acknowledged owned handle', () => {
  const m=opened(); assert.deepEqual(m.finish('complete'),{action:'stop_owned_lab_session',identity:id,handle});
  m.stopped({kind:'stopped',identity:id,handle});
  assert.deepEqual(m.snapshot(),{state:'closed',outcome:'workload_completed',cleanup:'confirmed_by_backend'});
  assert.throws(()=>m.finish('complete'));
});
test('each unknown preflight fact refuses before start', () => {
  for (const key of Object.keys(ready)) for (const value of [false,undefined,1]) {
    const m=new LabSessionModel(id); m.preflight({...ready,[key]:value});
    assert.equal(m.snapshot().state,'refused'); assert.throws(()=>m.begin());
  }
});
test('collision, ambiguous start and unowned or malformed acknowledgements cannot stop', () => {
  for (const result of [null,{kind:'collision'},{kind:'started',identity:other,handle},{kind:'started',identity:id,handle:'bad'}]) {
    const m=new LabSessionModel(id); m.preflight(ready); m.begin(); m.started(result);
    assert.deepEqual(m.snapshot(),{state:'ownership_unknown',outcome:'unknown',cleanup:'unknown'});
    assert.throws(()=>m.finish('failure'));
  }
});
test('timeout and workload failure remain unknown even after confirmed stop', () => {
  for (const reason of ['timeout','failure']) {
    const m=opened(); m.finish(reason); m.stopped({kind:'stopped',identity:id,handle});
    assert.deepEqual(m.snapshot(),{state:'closed',outcome:'unknown',cleanup:'confirmed_by_backend'});
  }
});
test('bad or failed stop acknowledgement does not claim cleanup or retry', () => {
  for (const result of [null,{kind:'stopped',identity:other,handle},{kind:'stopped',identity:id,handle:other},{kind:'failed',identity:id,handle}]) {
    const m=opened(); m.finish('complete'); m.stopped(result);
    assert.deepEqual(m.snapshot(),{state:'cleanup_unknown',outcome:'unknown',cleanup:'unknown'});
    assert.throws(()=>m.stopped({kind:'stopped',identity:id,handle}));
  }
});
test('controller loss never guesses ownership or produces a stop', () => {
  for (const phase of ['starting','running','stopping']) {
    const m=new LabSessionModel(id); m.preflight(ready); m.begin();
    if (phase!=='starting') m.started({kind:'started',identity:id,handle});
    if (phase==='stopping') m.finish('complete');
    assert.equal(m.controllerLost(),undefined);
    assert.deepEqual(m.snapshot(),{state:'ownership_unknown',outcome:'unknown',cleanup:'unknown'});
    assert.throws(()=>m.finish('failure'));
  }
});
test('out of order transitions and invalid identities fail', () => {
  assert.throws(()=>new LabSessionModel('NT Kernel Logger'));
  const m=new LabSessionModel(id); assert.throws(()=>m.started({kind:'started',identity:id,handle}));
  assert.throws(()=>m.finish('complete')); assert.equal(m.snapshot().state,'new');
});
