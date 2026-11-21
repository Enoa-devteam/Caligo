#![cfg(feature = "research")]

use caligo_bridge::qq_v8::{
    D8_POLL_JS, D8_START_JS, D8_STOP_JS, build_callback_observations_js, build_send_js,
};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn callback_observations_are_listener_bound_and_do_not_mutate_business_state() {
    let scripts = serde_json::json!({
        "start": D8_START_JS, "poll": D8_POLL_JS, "stop": D8_STOP_JS,
        "first": build_send_js(2, "455", "PRIVATE_BODY_A", "233", "private-key-a", 1, 20000),
        "second": build_send_js(2, "455", "PRIVATE_BODY_B", "233", "private-key-b", 2, 20000),
        "read1": build_callback_observations_js("nt_0", 1).unwrap(),
        "read2": build_callback_observations_js("nt_0", 2).unwrap(),
        "stopped_read": caligo_bridge::qq_v8::build_stopped_observation_js(&caligo_bridge::stop_observation::StopIdentity {
            account:"233".into(),host_nonce:7,wrapper_session:"nt_0".into(),listener_generation:1,
        }).unwrap(),
    });
    let fixture = r#"
const assert = require('node:assert/strict');
const vm = require('node:vm');
const tick = () => new Promise(resolve => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((a,b) => { resolve=a; reject=b; });
  return {resolve, reject, promise};
}
function harness(trace, failRemove=false, tidGetter=null) {
  let id=0, listener;
  const jobs=[], reads=new Map();
  const service={
    addKernelMsgListener(l) { listener=l; return 99; },
    removeKernelMsgListener(id) {
      assert.equal(id,99);
      listener.onRecvMsg(null);
      listener.onMsgInfoListUpdate(null);
      listener.onAddSendMsg(null);
      if (failRemove) throw Error('PRIVATE_REMOVE_ERROR');
      return 'ok';
    },
    generateMsgUniqueId() { return 'PRIVATE_COOKIE_' + ++id; },
    sendMsg(_, peer, elements) {
      const submit=deferred(), read=deferred();
      const m={...peer, peerUin:'455', msgId:'PRIVATE_NATIVE_' + id, msgSeq:'1',
        msgRandom:'1', msgTime:'1', msgType:0, subMsgType:0, senderUid:'u_self',
        senderUin:'233', sendStatus:2, elements:[{...elements[0],
          textElement:{...elements[0].textElement,atType:0}}]};
      reads.set(m.msgId,read);
      jobs.push({submit,read,m});
      return submit.promise;
    },
    getMsgsByMsgId(_, ids) { return reads.get(ids[0]).promise; },
  };
  const session={getSessionId:()=>1,getMsgService:()=>service,
    getMSFService:()=>({getServerTime:()=> '1'})};
  const context=vm.createContext({Buffer,caligoSendTrace:trace,caligoStopProbe:true,
    caligoThreadObserversReady:tidGetter!==null,caligoTidGetterReady:tidGetter!==null,__caligo_current_tid:tidGetter,
    process:{_linkedBinding:()=>({NodeIQQNTWrapperSession:{getNTWrapperSession:()=>session}})}});
  const run = script => vm.runInContext(script,context);
  const start = () => {
    assert.equal(run(s.start),'ARMED sid=nt_0 lidRet=99');
    return context.__caligo_res;
  };
  const fire = script => { assert.equal(JSON.parse(run(script)).fired,true); return jobs.at(-1); };
  const match = ({m}) => { listener.onAddSendMsg(m); listener.onMsgInfoListUpdate([m]); };
  const snapshot = () => JSON.parse(run(s.read1));
  return {context,start,fire,match,run,snapshot};
}
const readOK = ({m}) => ({result:0,msgList:[m]});
const shape = ({kind,seq,current,phase,association}) => [kind,seq,current,phase,association];
const done = ['stop_begin','recv','update','add','stop_end'];
const state = R => JSON.stringify([R.sends,R.sendResults,R.recv,R.recvBytes,R.rejected,R.overflow]);
(async () => {
  const h=harness(true), r=h.start();
  const a=h.fire(s.first), b=h.fire(s.second);
  r.listener.onAddSendMsg({...b.m,guildId:'unmatched'});
  for (const j of [b,a]) {
    h.match(j);
    j.submit.resolve({result:0});
    j.read.resolve(readOK(j));
    await tick();
  }
  assert.equal(r.sendResults['private-key-a'].msgId,a.m.msgId);
  assert.equal(r.sendResults['private-key-b'].msgId,b.m.msgId);
  const snap=h.snapshot();
  assert.deepEqual(snap.events.map(shape),[
    ['add',0,null,'active','none'],
    ...[2,1].flatMap(seq=>[
      ['add',0,null,'active','none'], ['matched_add',seq,true,'active','matched'],
      ['update',0,null,'active','none'], ['matched_update',seq,true,'active','matched'],
      ['promise_fulfilled',seq,true,'active','closure'], ['readback_fulfilled',seq,true,'active','closure'],
    ]),
  ]);
  const before=state(r);
  assert.deepEqual(h.snapshot(),snap);
  assert.equal(state(r),before);
  h.run(s.poll);
  assert.deepEqual(h.snapshot(),snap);
  r.callbackObservations[0].requestId='PRIVATE_EXTRA';
  delete r.callbackObservations[1].kind;
  assert.equal(h.snapshot().overflow,true);
  assert.equal(h.snapshot().events[1].order,3);
  assert.doesNotMatch(h.run(s.read1),/PRIVATE|private-key|455|233/);

  for (const fail of [false,true]) {
    const h=harness(true,fail), old=h.start();
    const a=h.fire(s.first), b=h.fire(s.second);
    h.match(a); h.match(b);
    await tick();
    // A retired request in the captured table and a retained one.
    delete old.sends['private-key-b'];
    const before=state(old), n=old.callbackObservations.length;
    const stop=h.run(s.stop);
    assert.equal(stop,fail?'ERR:listener_remove_failed stop_probe=pass':'STOPPED sid=nt_0 remove=ok stop_probe=pass');
    const end=fail?'remove_failed':'stopped';
    assert.deepEqual(h.snapshot().events.slice(n).map(e=>e.kind),fail?[...done.slice(0,4),'stop_remove_failed']:done);
    assert.deepEqual(h.snapshot().events.slice(n).map(e=>e.phase),[...Array(4).fill('stopping'),end]);
    if (!fail) {
      h.start();
      h.fire(s.first);
      assert.equal(h.run(s.read1),'UNAVAILABLE');
    }
    a.submit.resolve({result:0}); a.read.reject(Error('PRIVATE_READ_ERROR'));
    b.submit.reject(Error('PRIVATE_SUBMIT_ERROR')); b.read.resolve(readOK(b));
    old.listener.onRecvMsg(null);
    await tick();
    assert.equal(state(old),before);
    const late=Array.from(old.callbackObservations.slice(n+5),shape);
    assert.deepEqual(late.sort(),[
      ['recv',0,null,end,'none'], ['promise_fulfilled',1,true,end,'closure'],
      ['promise_rejected',2,false,end,'closure'], ['readback_rejected',1,true,end,'closure'],
      ['readback_fulfilled',2,false,end,'closure'],
    ].sort());
    assert.ok(old.callbackObservations.every((e,i)=>e.generation===1 && e.order===i+1));
    if (!fail) assert.deepEqual(JSON.parse(h.run(s.read2)),{generation:2,overflow:false,events:[]});
  }

  {
    // Model a later owner tick reading the same stopped listener, without
    // rearming START or replacing its global root. These are synthetic replies.
    const h=harness(true,false,()=>77), r=h.start();
    const a=h.fire(s.first);h.match(a);await tick();
    r.voiceAccount='233';
    assert.equal(h.run(s.stopped_read),'UNAVAILABLE','reader rejects an active listener');
    const before=state(r);
    assert.equal(h.run(s.stop),'STOPPED sid=nt_0 remove=ok stop_probe=pass');
    assert.deepEqual(JSON.parse(h.run(s.stopped_read)),h.snapshot());
    r.voiceAccount='234';assert.equal(h.run(s.stopped_read),'UNAVAILABLE','wrong account is rejected');
    r.voiceAccount='233';
    const immediate=h.snapshot();const end=immediate.events.at(-1);
    assert.equal(end.kind,'stop_end');assert.equal(end.phase,'stopped');
    a.submit.resolve({result:0});a.read.resolve(readOK(a));await tick();await tick();
    const later=h.snapshot();
    assert.deepEqual(later.events.slice(0,immediate.events.length),immediate.events);
    assert.deepEqual(later.events.slice(immediate.events.length).map(shape).sort(),[
      ['promise_fulfilled',1,true,'stopped','closure'],['readback_fulfilled',1,true,'stopped','closure'],
    ].sort());
    assert.ok(later.events.slice(immediate.events.length).every(e=>e.callback_tid===77 && e.order>end.order));
    assert.ok(later.events.slice(immediate.events.length).every(e=>e.text_state==='unchanged'));
    assert.equal(state(r),before,'later observation and replies do not change stopped business state');
    assert.deepEqual(h.snapshot(),later,'repeated read is non-destructive');
    assert.equal(h.context.__caligo_res,undefined);
    assert.equal(h.context.__caligo_d8,r);
  }
  {
    // Holder probe: the stopped reader needs __caligo_d8, while a pending
    // promise closure still reaches the same R after that global is cleared.
    const h=harness(true,false,()=>77), r=h.start();
    const job=h.fire(s.first);
    assert.match(h.run(s.stop),/^STOPPED/);
    r.voiceAccount='233';
    assert.ok(h.snapshot().events.length>0);
    delete h.context.__caligo_d8;
    assert.equal(h.run('delete globalThis.__caligo_d8'),true);
    assert.equal(h.run(s.stopped_read),'UNAVAILABLE');
    job.submit.resolve({result:0});job.read.resolve(readOK(job));
    await tick();await tick();
    assert.ok(r.callbackObservations.some(e=>e.phase==='stopped'),
      'pending promise closures still reach the retained object');
  }
  for (const mode of ['changed','unverified']) {
    const h=harness(true,false,()=>77), r=h.start();
    const job=h.fire(s.first);
    const pending=r.sends['private-key-a'];
    assert.match(h.run(s.stop),/^STOPPED/);
    if (mode==='unverified') r.sendResults=null;
    job.submit.resolve({get result() {
      if (mode==='changed') pending.promiseStatus='test-only mutation';
      return 0;
    }});
    await tick();
    const row=h.snapshot().events.at(-1);
    assert.equal(row.kind,'promise_fulfilled');
    assert.equal(row.phase,'stopped');
    assert.equal(row.text_state,mode);
    assert.doesNotMatch(h.run(s.read1),/test-only mutation|PRIVATE/);
  }
  {
    const h=harness(true);let calls=0;
    h.context.caligoThreadObserversReady=true;
    h.context.caligoTidGetterReady=false;
    Object.defineProperty(h.context,'__caligo_current_tid',{value:()=>{calls++;return 1234},configurable:false});
    const r=h.start();r.listener.onRecvMsg([]);
    assert.equal(calls,0,'failed getter installation cannot trust an existing callable');
    assert.equal(h.snapshot().events[0].callback_tid,null);
  }
  for(const getter of [()=>77,()=>4294967295,()=>0,()=>1.5,()=> '77',()=>{throw Error('PRIVATE_TID_ERROR')}]) {
    const h=harness(true,false,getter), r=h.start();
    const job=h.fire(s.first);h.match(job);job.submit.resolve({result:0});job.read.resolve(readOK(job));await tick();
    assert.equal(r.sendResults['private-key-a'].status,'done');
    const rows=h.snapshot().events;
    const sampled=(()=>{try{return getter()}catch{return null}})();
    const expected=Number.isInteger(sampled)&&sampled>0&&sampled<=4294967295?sampled:null;
    assert.ok(rows.length>0 && rows.every(e=>e.callback_tid===expected));
    assert.doesNotMatch(h.run(s.read1),/PRIVATE/);
  }
  {
    const h=harness(true), r=h.start();
    const fail=()=> { throw Error('PRIVATE_DIAGNOSTIC'); };
    Object.defineProperty(r,'observeCallback',{get:fail});
    Object.defineProperty(r,'observeStopPhase',{get:fail});
    Object.defineProperty(r,'noteCallbackThread',{get:fail});
    r.callbackObservations.push=fail;
    const a=h.fire(s.first);
    h.match(a); a.submit.resolve({result:0}); a.read.resolve(readOK(a));
    await tick();
    assert.deepEqual(JSON.parse(JSON.stringify(r.sendResults['private-key-a'])),{status:'done',msgId:a.m.msgId});
    assert.equal(r.callbackObservationsOverflow,true);
    assert.equal(h.run(s.stop),'STOPPED sid=nt_0 remove=ok stop_probe=pass');
  }
  {
    const h=harness(true), r=h.start();
    for (let i=0;i<128;i++) r.listener.onRecvMsg([]);
    assert.equal(h.snapshot().overflow,false);
    h.run(s.poll);
    r.listener.onRecvMsg([]);
    const full=h.snapshot();
    assert.equal(full.events.length,128);
    assert.equal(full.overflow,true);
    assert.ok(full.events.every((e,i)=>e.order===i+1));
    h.run(s.stop);
    assert.deepEqual(h.snapshot(),full);
    r.voiceAccount='233';
    const stopped=JSON.parse(h.run(s.stopped_read));
    assert.equal(stopped.overflow,true,'the stopped snapshot keeps the overflow flag');
    assert.equal(stopped.events.length,128);
    assert.deepEqual(stopped,full,'the stopped reader returns the same overfull batch');
  }
  {
    const h=harness(false), r=h.start();
    assert.equal(r.callbackObservations,undefined);
    const a=h.fire(s.first);
    h.match(a); a.submit.resolve({result:0}); a.read.resolve(readOK(a));
    await tick();
    assert.equal(r.sendResults['private-key-a'].status,'done');
    assert.equal(h.run(s.read1),'UNAVAILABLE');
    assert.equal(h.run(s.stop),'STOPPED sid=nt_0 remove=ok stop_probe=pass');
    assert.equal(r.callbackObservations,undefined);
  }
})().catch(error=>{console.error(error);process.exitCode=1;});
"#;
    let mut child = Command::new("node")
        .arg("--unhandled-rejections=strict")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Node.js is required for the JavaScript fixture");
    write!(child.stdin.take().unwrap(), "const s={scripts};\n{fixture}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
