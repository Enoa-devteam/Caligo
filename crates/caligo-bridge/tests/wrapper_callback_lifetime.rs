use caligo_bridge::qq_v8::{build_send_js, D8_START_JS, D8_STOP_JS};
use std::io::Write;

#[test]
fn stopped_listener_and_pending_remain_rooted_across_gc_without_state_mutation() {
    let send = build_send_js(1, "20002", "lifetime fixture", "10001", "request", 42, 5_000);
    let fixture = r#"
const assert=require('node:assert/strict'),vm=require('node:vm');
const start=__START__,stop=__STOP__,send=__SEND__;
assert.equal(typeof global.gc,'function');
const tick=()=>new Promise(r=>setImmediate(r));
const record=status=>({msgId:'native-lifetime',msgSeq:'1',msgRandom:'2',msgTime:'1794567890',guildId:'cookie',
 chatType:1,msgType:2,subMsgType:0,peerUid:'u_peer',peerUin:'20002',senderUid:'u_self',senderUin:'10001',sendStatus:status,
 elements:[{elementType:1,textElement:{content:'lifetime fixture',atType:0}}]});
const make=mode=>{
 let listener=null,resolveSend=null,resolveQuery=null,removes=0,queries=0,downloads=0;
 const service={generateMsgUniqueId:()=> 'cookie',addKernelMsgListener(l){listener=l;return 7},
  removeKernelMsgListener(){removes++;listener=null;if(mode==='remove-fails')throw Error('fixture')},
  sendMsg(){return new Promise(r=>resolveSend=r)},
  getMsgsByMsgId(){queries++;return new Promise(r=>resolveQuery=r)}};
 const session={getSessionId:()=>1,getMsgService:()=>service,getMSFService:()=>({getServerTime:()=> 'time'}),
  getProfileService:()=>({getUidByUin:()=>new Map([['20002','u_peer']])})};
 const c={Buffer,Promise,Date,Map,JSON,String,Object,Array,Number,
  process:{_linkedBinding:()=>({NodeIQQNTWrapperSession:{getNTWrapperSession:()=>session}})}};
 vm.createContext(c);
 return {c,run:s=>vm.runInContext(s,c),service,session,
  resolveSend(){const r=resolveSend;resolveSend=null;r({result:0})},
  resolveQuery(){const r=resolveQuery;resolveQuery=null;r({result:0,msgList:[record(2)]})},
  noServiceListener:()=>listener===null,queries:()=>queries,removes:()=>removes,
  cleanup:()=>{downloads++},downloads:()=>downloads};
};
(async()=>{
 // GC is only a pressure input. Collection of an unrooted object within a
 // fixed number of calls is not guaranteed, so it is not an acceptance oracle.
 for(const mode of ['pending-send','pending-readback','remove-fails']){
  const f=make(mode);assert.match(f.run(start),/^ARMED/);assert.equal(JSON.parse(f.run(send)).fired,true);
  // WeakRefs are the test's only references to these objects outside production
  // globals; the fake service drops its listener when remove is called.
  let refs=(()=>{const R=f.c.__caligo_res;R.voiceDownload=f.cleanup;
   if(mode==='pending-readback'){R.listener.onAddSendMsg(record(1));R.listener.onMsgInfoListUpdate([record(2)]);}
   return {state:new WeakRef(R),listener:new WeakRef(R.listener),pending:new WeakRef(R.sends.request),
    };})();
  await tick();assert.equal(f.queries(),mode==='pending-readback'?1:0);
  const out=f.run(stop);
  if(mode==='remove-fails')assert.equal(out,'ERR:listener_remove_failed');else assert.match(out,/^STOPPED/);
  assert.equal(f.noServiceListener(),true);assert.equal(f.removes(),1);
  for(let i=0;i<4;i++){await tick();global.gc();}
  {
   const R=refs.state.deref();assert.ok(R,'production alias keeps stopped R alive');
   assert.equal(f.c.__caligo_d8,R);assert.equal(R.stopped,true);
   assert.equal(refs.listener.deref(),R.listener);assert.equal(refs.pending.deref(),R.sends.request);
   // These identity checks document outgoing references, not exclusive roots:
   // the fake host itself also owns its service and session.
   assert.equal(R.ms,f.service);assert.equal(R.session,f.session);
   if(mode==='remove-fails')assert.equal(f.c.__caligo_res,R);else assert.equal(f.c.__caligo_res,undefined);
   const before=JSON.stringify({recv:R.recv,bytes:R.recvBytes,rejected:R.rejected,overflow:R.overflow,
    shapes:R.imageShapes,sends:R.sends,results:R.sendResults});
   // Synthetic calls model a retained native snapshot, never a real QQ callback.
   refs.listener.deref().onRecvMsg([record(2)]);refs.listener.deref().onAddSendMsg(record(1));
   refs.listener.deref().onMsgInfoListUpdate([record(2)]);
   f.resolveSend();if(mode==='pending-readback')f.resolveQuery();await tick();await tick();
   assert.equal(JSON.stringify({recv:R.recv,bytes:R.recvBytes,rejected:R.rejected,overflow:R.overflow,
    shapes:R.imageShapes,sends:R.sends,results:R.sendResults}),before);
   assert.equal(f.queries(),mode==='pending-readback'?1:0,'stopped callbacks cannot start another host query');
   R.listener.onRichMediaDownloadComplete({status:0});assert.equal(f.downloads(),1,'synthetic late media callback routing stays reachable');
  }
 }
})().catch(e=>{console.error(e);process.exitCode=1});
"#
    .replace("__START__", &serde_json::to_string(D8_START_JS).unwrap())
    .replace("__STOP__", &serde_json::to_string(D8_STOP_JS).unwrap())
    .replace("__SEND__", &serde_json::to_string(&send).unwrap());
    let mut child = std::process::Command::new("node")
        .args(["--expose-gc", "--unhandled-rejections=strict"])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Node.js is required for callback lifetime contracts");
    child.stdin.take().unwrap().write_all(fixture.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
