const assert = require('node:assert/strict');
const scripts = JSON.parse(require('node:fs').readFileSync(0, 'utf8'));
const tick = () => new Promise(resolve => setImmediate(resolve));
const run = source => JSON.parse(eval(source));
let listeners, removed, calls, reply, removeFails, registration;
function reset() {
  listeners = []; removed = []; calls = 0; removeFails = false; registration = undefined;
  reply = () => Promise.resolve({result:0,errMsg:''});
  const service = {
    addKernelGroupListener(listener) { listeners.push(listener); return registration === undefined ? listeners.length : registration; },
    removeKernelGroupListener(id) { if (removeFails) throw Error('private-path-secret'); removed.push(id); },
    getGroupDetailInfo(group, source) { assert.equal(source,2); calls++; return reply(group); }
  };
  globalThis.__caligo_d8 = {sid:'nt_contract',session:{getGroupService:()=>service}};
}
function detail(group='30003') { return {groupCode:group,groupName:'合成群',memberNum:3,maxMemberNum:200,credential:'private-secret'}; }
(async () => {
  reset();
  let accept;
  reply = group => { listeners[0].onGroupDetailInfoChange(detail(group)); return new Promise(resolve=>accept=resolve); };
  assert.equal(run(scripts.start).status,'pending');
  assert.equal(run(scripts.poll).status,'pending');
  accept({result:0}); await tick();
  const success=run(scripts.poll);
  assert.equal(success.status,'success'); assert.equal(success.data.data.group_name,'合成群');
  assert(!JSON.stringify(success).includes('private-secret')); assert.deepEqual(removed,[1]);
  // Sequential groups must not reuse the first request's group or listener closure.
  reply=()=>Promise.resolve({result:0});
  assert.equal(run(scripts.next).status,'pending'); await tick();
  listeners[0].onGroupDetailInfoChange(detail('30004'));
  listeners[1].onGroupDetailInfoChange(detail('30003'));
  assert.equal(run(scripts.nextPoll).status,'pending');
  listeners[1].onGroupDetailInfoChange(detail('30004'));
  assert.equal(run(scripts.nextPoll).status,'success'); assert.deepEqual(removed,[1,2]);

  reset();
  reply=group=>{listeners[0].onGroupDetailInfoChange(detail(group));return Promise.resolve({result:7});};
  run(scripts.start); await tick(); assert.equal(run(scripts.poll).status,'rejected'); assert.deepEqual(removed,[1]);
  reset(); reply=()=>Promise.resolve({});
  run(scripts.start); await tick(); assert.equal(run(scripts.poll).status,'invalid_response');
  reset(); run(scripts.start); await tick(); removeFails=true;
  listeners[0].onGroupDetailInfoChange(detail());
  assert.equal(run(scripts.poll).status,'invalid_response');
  assert.equal(run(scripts.next).status,'rejected'); assert.equal(calls,1);

  reset(); run(scripts.start); await tick();
  assert.equal(run(scripts.cancel).status,'cancelled');
  listeners[0].onGroupDetailInfoChange(detail());
  assert.equal(run(scripts.poll).status,'rejected');
  assert.equal(run(scripts.next).status,'rejected'); assert.equal(calls,1); assert.deepEqual(removed,[1]);
  reset(); run(scripts.start); await tick();
  globalThis.__caligo_d8.queryPending.get('detail').deadline=0;
  listeners[0].onGroupDetailInfoChange(detail());
  assert.equal(run(scripts.poll).status,'timeout');
  assert.equal(run(scripts.next).status,'rejected'); assert.equal(calls,1);
  reset(); run(scripts.start); await tick();
  assert.equal(run(scripts.rebind).status,'rejected'); await tick();
  listeners[0].onGroupDetailInfoChange(detail());
  assert.equal(calls,1); assert.deepEqual(removed,[1]);
  reset(); run(scripts.start); await tick(); listeners[0].onGroupDetailInfoChange(detail());
  assert.equal(run(scripts.cancel).status,'cancelled');
  assert.equal(run(scripts.next).status,'pending'); await tick();
  listeners[1].onGroupDetailInfoChange(detail('30004'));
  assert.equal(run(scripts.nextPoll).status,'success');
  reset(); run(scripts.start); await tick(); listeners[0].onGroupDetailInfoChange(detail());
  assert.equal(run(scripts.rebind).status,'pending'); await tick();
  assert.equal(calls,2); assert.deepEqual(removed,[1]);
  reset(); run(scripts.start); await tick(); globalThis.__caligo_d8.stopped=true;
  listeners[0].onGroupDetailInfoChange(detail());
  assert.equal(globalThis.__caligo_d8.queryPending.get('detail').status,'pending');
  for (const id of ['9007199254740993', {private:'secret'}, -1]) {
    reset(); registration=id;
    const result=run(scripts.start);
    assert.equal(result.status,'invalid_response');
    assert.equal(result.reason,'Error: group detail listener registration');
    assert.deepEqual(result.diagnostics,[{stage:'group_detail_registration',fields:{value:typeof id}}]);
    assert.equal(calls,0); assert.deepEqual(removed,[]);
    assert.equal(run(scripts.next).status,'rejected');
  }

  reset(); registration='18446744073709551615';
  assert.equal(run(scripts.pilotStart).status,'pending'); await tick();
  listeners[0].onGroupDetailInfoChange(detail());
  assert.equal(run(scripts.poll).status,'success');
  assert.deepEqual(removed,['18446744073709551615']);

  reset(); registration='9007199254740993';
  assert.equal(run(scripts.pilotStart).status,'pending');
  assert.equal(run(scripts.cancel).status,'cancelled');
  assert.deepEqual(removed,['9007199254740993']);
  assert.equal(run(scripts.pilotNext).status,'rejected');

  reset(); registration='9'; removeFails=true;
  assert.equal(run(scripts.pilotStart).status,'pending'); await tick();
  listeners[0].onGroupDetailInfoChange(detail());
  const cleanupFailure=run(scripts.poll);
  assert.equal(cleanupFailure.status,'invalid_response');
  assert.equal(cleanupFailure.reason,'group detail listener cleanup');
  assert.equal(run(scripts.pilotNext).status,'rejected');

  for (const id of ['', '0', '01', '42\n', '+1', '42rest', '18446744073709551616', 1, 1n]) {
    reset(); registration=id;
    const result=run(scripts.pilotStart);
    assert.equal(result.status,'invalid_response');
    assert.equal(result.reason,'Error: group detail listener registration');
    assert.equal(calls,0); assert.deepEqual(removed,[]);
    assert.equal(run(scripts.pilotNext).status,'rejected');
  }
  console.log('group detail acceptance, per-request listener, cleanup and late callback isolation PASS');
})().catch(e=>{console.error(e);process.exitCode=1;});
