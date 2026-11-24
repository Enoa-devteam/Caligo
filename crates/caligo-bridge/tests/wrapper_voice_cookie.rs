use caligo_bridge::qq_v8::{build_voice_send_js, D8_POLL_JS, D8_START_JS, D8_STOP_JS};

#[test]
fn wrapper_voice_send_rejects_cookie_reuse_after_settlement() {
    use std::io::Write;
    let hex = "02232153494c4b5f56330300445566";
    let silk: Vec<u8> = (0..hex.len()).step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i+2], 16).unwrap()).collect();
    let one = build_voice_send_js(1,"10001","20002","voice-one",1,5000,&silk).unwrap();
    let two = build_voice_send_js(1,"10001","20002","voice-two",2,5000,&silk).unwrap();
    let fixture = r#"
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),
  os=require('node:os'),crypto=require('node:crypto');
const start=__START__,poll=__POLL__,stop=__STOP__,one=__ONE__,two=__TWO__;
const caligoVoiceAccount='10001';
const root=fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()),'caligo-voice-reuse-'));
const bytes=Buffer.from('__HEX__','hex'),md5=crypto.createHash('md5').update(bytes).digest('hex');
const dest=path.join(root,md5+'.silk');
let listener,sends=0,reads=0;
const row=status=>({msgId:'12345',msgSeq:'1',msgRandom:'2',msgTime:'1794567890',
  guildId:'reused-cookie',chatType:1,msgType:2,subMsgType:0,peerUid:'u_peer',peerUin:'20002',
  senderUid:'u_self',senderUin:'10001',sendStatus:status,
  elements:[{elementType:4,elementId:'67890',pttElement:{fileSize:String(bytes.length),
    md5HexStr:md5}}]});
const service={generateMsgUniqueId:()=> 'reused-cookie',addKernelMsgListener(l){listener=l;return 7;},
  removeKernelMsgListener:()=>true,getRichMediaFilePathForGuild:()=>dest,
  sendMsg(){sends++;return Promise.resolve({result:0});},
  getMsgsByMsgId(){reads++;return Promise.resolve({result:0,msgList:[row(2)]});}};
const session={getSessionId:()=>1,getMsgService:()=>service,getMSFService:()=>({getServerTime:()=> 'time'}),
  getProfileService:()=>({getUidByUin:()=>new Map([['20002','u_peer']])})};
process._linkedBinding=()=>({NodeIQQNTWrapperSession:{getNTWrapperSession:()=>session}});
const tick=()=>new Promise(r=>setImmediate(r));
(async()=>{
  assert.match(eval(start),/^ARMED/);assert.equal(JSON.parse(eval(one)).fired,true);
  listener.onAddSendMsg(row(1));listener.onMsgInfoListUpdate([row(2)]);await tick();await tick();
  assert.equal(JSON.parse(eval(poll)).sendResults['voice-one'].status,'done');
  assert.equal(Object.hasOwn(globalThis.__caligo_res.sends,'voice-one'),false);
  const second=JSON.parse(eval(two));
  listener.onAddSendMsg(row(1));listener.onMsgInfoListUpdate([row(2)]);await tick();await tick();
  const later=JSON.parse(eval(poll));
  assert.notEqual(later.sendResults['voice-two']?.status,'done','old voice notification must not confirm later send');
  assert.deepEqual(second,{err:'invalid_cookie'});assert.equal(sends,1);assert.equal(reads,1);
  eval(stop);
})().catch(e=>{console.error(e);process.exitCode=1;}).finally(()=>fs.rmSync(root,{recursive:true,force:true}));
"#.replace("__START__",&serde_json::to_string(D8_START_JS).unwrap())
 .replace("__POLL__",&serde_json::to_string(D8_POLL_JS).unwrap())
 .replace("__STOP__",&serde_json::to_string(D8_STOP_JS).unwrap())
 .replace("__ONE__",&serde_json::to_string(&one).unwrap())
 .replace("__TWO__",&serde_json::to_string(&two).unwrap()).replace("__HEX__",hex);
    let mut child=std::process::Command::new("node").arg("--unhandled-rejections=strict")
        .stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(fixture.as_bytes()).unwrap();
    let output=child.wait_with_output().unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
}

#[test]
fn wrapper_voice_cookie_history_survives_rearm_and_fails_closed() {
    use std::io::Write;
    let hex="02232153494c4b5f56330300445566";
    let silk: Vec<u8>=(0..hex.len()).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16).unwrap()).collect();
    let send=build_voice_send_js(1,"10001","20002","voice",1,5000,&silk).unwrap();
    let fixture=r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs'),
 path=require('node:path'),os=require('node:os'),crypto=require('node:crypto'),send=__SEND__;
const bytes=Buffer.from('__HEX__','hex'),md5=crypto.createHash('md5').update(bytes).digest('hex');
const root=fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()),'caligo-voice-cookie-'));
let cookie='first',calls=0;
const service={generateMsgUniqueId:()=>cookie,getRichMediaFilePathForGuild:()=>path.join(root,md5+'.silk'),
 sendMsg(){calls++;return new Promise(()=>{});}};
const session={getMSFService:()=>({getServerTime:()=> 'time'}),
 getProfileService:()=>({getUidByUin:()=>new Map([['20002','u_peer']])})};
const resource=()=>({voiceAccount:'10001',stopped:false,sends:{},sendResults:{},ms:service,session});
function context(){const c={__caligo_res:resource(),process,Buffer,Promise,Date,Map,JSON,String,Object,Array,Number};vm.createContext(c);return c;}
const run=c=>JSON.parse(vm.runInContext(send,c));
try {
 const c=context();assert.equal(run(c).fired,true);assert.equal(calls,1);
 const descriptor=Object.getOwnPropertyDescriptor(c,'__caligo_voice_cookie_guard_v1');
 assert.equal(descriptor.writable,false);assert.equal(descriptor.configurable,false);
 c.__caligo_res=resource();assert.deepEqual(run(c),{err:'invalid_cookie'});assert.equal(calls,1);
 for(let i=0;i<65535;i++) assert.equal(descriptor.value('filled-'+i),'ok');
 assert.equal(descriptor.value('first'),'invalid_cookie');assert.equal(descriptor.value('x'.repeat(257)),'invalid_cookie');
 cookie='new-cookie';assert.deepEqual(run(c),{err:'cookie_history_exhausted'});assert.equal(calls,1);
 for(const kind of ['null','mutable','accessor']) {
   const d=context();
   if(kind==='null') d.__caligo_voice_cookie_guard_v1=null;
   else if(kind==='mutable') d.__caligo_voice_cookie_guard_v1=()=> 'ok';
   else Object.defineProperty(d,'__caligo_voice_cookie_guard_v1',{get(){throw Error('getter invoked');},configurable:true});
   assert.deepEqual(run(d),{err:'cookie_history_unavailable'});assert.equal(calls,1);
 }
} finally {fs.rmSync(root,{recursive:true,force:true});}
"#.replace("__SEND__",&serde_json::to_string(&send).unwrap()).replace("__HEX__",hex);
    let mut child=std::process::Command::new("node").arg("--unhandled-rejections=strict")
        .stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(fixture.as_bytes()).unwrap();
    let output=child.wait_with_output().unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
}
