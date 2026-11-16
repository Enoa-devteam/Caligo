use caligo_bridge::qq_v8::{build_image_send_js, D8_POLL_JS, D8_START_JS, D8_STOP_JS};

#[test]
fn wrapper_png_send_keeps_cache_and_requires_fingerprint_bound_readback() {
    use std::io::Write;
    let hex="89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63105432fe0f000214016604e0de7f0000000049454e44ae426082";
    let png: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let quote = |text: &str| serde_json::to_string(text).unwrap();
    let send = build_image_send_js(1, "10001", "20002", "image-7", 42, 5000, &png).unwrap();
    let send_group = build_image_send_js(2, "10001", "30003", "image-7", 42, 5000, &png).unwrap();
    assert!(build_image_send_js(
        1,
        "10001",
        "20002",
        "image-7",
        42,
        5000,
        &png[..png.len() - 1]
    )
    .is_none());
    let script = format!(
        "const start={}, poll={}, stop={}, send={}, sendGroup={};\n",
        quote(D8_START_JS),
        quote(D8_POLL_JS),
        quote(D8_STOP_JS),
        quote(&send),
        quote(&send_group),
    ) + r#"
const assert = require('node:assert/strict');
const fs = require('node:fs');
const crypto = require('node:crypto');
const path = require('node:path');
const os = require('node:os');
const root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'caligo-image-test-'));
const bytes = Buffer.from('89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63105432fe0f000214016604e0de7f0000000049454e44ae426082','hex');
const md5 = crypto.createHash('md5').update(bytes).digest('hex');
const msgId = '90071992547409931237', elementId = '90071992547409931238';
const caligoVoiceAccount = '10001';
let listener, submits, reads, storageCalls, settle, mode, dest, cookie, now = 1000;
let chatType=1, peerUid='u_peer', peerUin='20002';
Date.now = () => now;
const flush = () => new Promise(resolve=>setImmediate(resolve));
function record(status, guildId=cookie, fingerprint=md5, id=elementId) {
  return {msgId,msgSeq:'1',msgRandom:'2',msgTime:'1794567890',guildId,chatType,msgType:2,
    subMsgType:0,peerUid,peerUin,senderUid:'u_self',senderUin:'10001',
    sendStatus:status,elements:[{elementType:2,elementId:id,
      picElement:{fileSize:String(bytes.length),md5HexStr:fingerprint,sourcePath:dest,
        picType:1000,picSubType:0,picWidth:1,picHeight:1}}]};
}
const service = {
  addKernelMsgListener(value) { listener=value; return 8; },
  removeKernelMsgListener() { return true; },
  generateMsgUniqueId(chat,time) { assert.equal(chat,chatType); assert.equal(time,'qq-time'); return cookie; },
  getRichMediaFilePathForGuild(args) {
    storageCalls++;
    assert.deepEqual(args,{md5HexStr:md5,fileName:md5+'.png',elementType:2,elementSubType:0,
      thumbSize:0,needCreate:true,downloadType:1,file_uuid:''});
    return dest;
  },
  sendMsg(id,peer,[element],options) {
    submits++;
    assert.equal(globalThis.__caligo_res.sends['image-7'].wireSeq,42);
    assert.equal(id,'0'); assert.equal(options.size,0);
    assert.deepEqual(peer,{chatType,peerUid,guildId:cookie});
    assert.equal(element.elementType,2);
    assert.equal(element.picElement.md5HexStr,md5);
    assert.equal(element.picElement.fileSize,String(bytes.length));
    assert.equal(element.picElement.picType,1000);
    assert.equal(element.picElement.sourcePath,dest);
    assert.equal(element.picElement.picWidth,1);
    assert.equal(element.picElement.original,true);
    assert.deepEqual(fs.readFileSync(dest),bytes);
    if (mode === 'throw') throw new Error('private cache path: '+dest);
    return new Promise(resolve=>{ settle=resolve; });
  },
  getMsgsByMsgId(peer,ids) {
    reads++;
    assert.deepEqual(ids,[msgId]);
    assert.equal(peer.guildId,cookie);
    return {result:0,msgList:[record(2,'readback-guild-id',md5,mode==='bad_readback' ? '999' : elementId)]};
  }
};
const session = {getSessionId:()=>1,getMsgService:()=>service,
  getMSFService:()=>({getServerTime:()=> 'qq-time'}),
  getProfileService:()=>({getUidByUin(source,ids) {
    assert.equal(chatType,1);
    assert.equal(source,'FriendsServiceImpl'); assert.deepEqual(ids,['20002']);
    return new Map([['20002','u_peer']]);
  }})};
process._linkedBinding = () => ({NodeIQQNTWrapperSession:{getNTWrapperSession:()=>session}});
function setup(kind, armedAccount='10001') {
  if (globalThis.__caligo_res) eval(stop);
  mode=kind; cookie='image-cookie-'+kind; submits=reads=storageCalls=0; settle=undefined; now=1000;
  dest=path.join(root,kind,md5+'.png');
  const caligoVoiceAccount=armedAccount;
  assert.ok(eval(start).startsWith('ARMED'));
}
const run = () => JSON.parse(eval(send));
function outcome(projected=false) {
  const batch=JSON.parse(eval(poll));
  assert.equal(batch.items.length, projected ? 1 : 0);
  if (projected) {
    assert.equal(batch.items[0].source,'add_send');
    assert.deepEqual(batch.items[0].record.elements,[{elementType:2,image:{chat_type:chatType,peer_uid:peerUid,sender_uin:'10001',message_id:msgId,element_id:elementId,
      size:bytes.length,md5,width:1,height:1,pic_type:1000}}]);
  }
  assert.ok(!JSON.stringify(batch).includes('sourcePath'));
  if (projected) process.stdout.write(JSON.stringify(batch)+'\n');
  return batch.sendResults['image-7'];
}
function notify() {
  listener.onAddSendMsg(record(1,'wrong-cookie'));
  listener.onAddSendMsg(record(1,cookie,'0'.repeat(32)));
  listener.onAddSendMsg(record(1));
  listener.onMsgInfoListUpdate([record(2,cookie,md5,'999'),record(2)]);
}
(async()=>{
  setup('unbound','30003');
  assert.deepEqual(run(),{err:'image_profile_unavailable'});
  assert.equal(storageCalls+submits,0);
  setup('conflict');
  fs.mkdirSync(path.dirname(dest),{recursive:true});
  const wrong=Buffer.alloc(bytes.length);
  fs.writeFileSync(dest,wrong);
  assert.deepEqual(run(),{err:'image_storage_mismatch'});
  assert.deepEqual(fs.readFileSync(dest),wrong);
  assert.equal(submits,0);
  setup('bad_path');
  dest=path.join(root,'bad_path','other.png');
  assert.deepEqual(run(),{err:'invalid_image_storage_path'});
  assert.equal(submits,0);
  assert.equal(fs.existsSync(dest),false);

  setup('good');
  assert.deepEqual(run(),{fired:true,mid:'image-7'});
  assert.deepEqual(run(),{err:'send_capacity_or_duplicate'});
  notify(); await flush();
  assert.equal(outcome().status,'pending');
  settle({result:0,msgId:'promise-id'}); await flush();
  assert.deepEqual(outcome(true),{status:'done',msgId});
  assert.equal(submits,1); assert.equal(reads,1);
  assert.deepEqual(fs.readFileSync(dest),bytes);

  setup('group');
  chatType=2; peerUid=peerUin='30003';
  assert.equal(JSON.parse(eval(sendGroup)).fired,true);
  listener.onAddSendMsg({...record(1),chatType:1});
  listener.onMsgInfoListUpdate([record(2)]);
  await flush();
  assert.equal(reads,0);
  notify(); settle({result:0}); await flush();
  assert.deepEqual(outcome(true),{status:'done',msgId});
  assert.equal(submits,1); assert.equal(reads,1);
  chatType=1; peerUid='u_peer'; peerUin='20002';

  setup('bad_readback');
  assert.equal(run().fired,true);
  settle({result:0}); await flush();
  assert.equal(outcome().status,'accepted');
  notify(); await flush();
  assert.deepEqual(outcome(),{status:'unknown',reason:'exact_query_identity_mismatch'});
  assert.equal(submits,1);

  setup('ambiguous');
  assert.equal(run().fired,true);
  listener.onAddSendMsg(record(1));
  listener.onAddSendMsg({...record(1),msgId:'999'});
  settle({result:0}); await flush();
  assert.equal(outcome().status,'unknown');
  assert.equal(reads,0);

  setup('throw');
  assert.equal(run().fired,true);
  assert.deepEqual(outcome(),{status:'unknown',reason:'image_submit_unknown'});
  assert.equal(submits,1); assert.equal(fs.existsSync(dest),true);

  setup('timeout');
  assert.equal(run().fired,true);
  now=7000;
  assert.deepEqual(outcome(),{status:'unknown',reason:'timeout'});
  settle({result:0}); await flush();
  assert.equal(outcome(),undefined);
  assert.ok(eval(stop).startsWith('STOPPED'));
  notify(); await flush();
  assert.equal(reads,0); assert.equal(fs.existsSync(dest),true);
})().catch(error=>{console.error(error);process.exitCode=1;})
  .finally(()=>fs.rmSync(root,{recursive:true,force:true}));
"#;
    let mut node = std::process::Command::new("node")
        .arg("--unhandled-rejections=strict")
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    node.stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = node.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = String::from_utf8(output.stdout).unwrap();
    assert_eq!(lines.lines().count(), 2);
    for line in lines.lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        let binding = caligo_model::wrapper_message::WrapperBinding {
            account: "10001".into(),
            host_nonce: 9,
            session_generation: 1,
            wrapper_session: value["sid"].as_str().unwrap().into(),
        };
        let batch = caligo_model::wrapper_message::decode_d8_batch(line, &binding).unwrap();
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].image().unwrap().pic_type, 1000);
        assert_eq!(batch.events[0].record.sender_uin, "10001");
    }
}

#[test]
fn wrapper_image_send_rejects_cookie_reuse_after_settlement() {
    use std::io::Write;
    let hex = "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63105432fe0f000214016604e0de7f0000000049454e44ae426082";
    let png: Vec<u8> = (0..hex.len()).step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i+2], 16).unwrap()).collect();
    let one = build_image_send_js(1,"10001","20002","image-one",1,5000,&png).unwrap();
    let two = build_image_send_js(1,"10001","20002","image-two",2,5000,&png).unwrap();
    let fixture = r#"
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),
  os=require('node:os'),crypto=require('node:crypto');
const start=__START__,poll=__POLL__,stop=__STOP__,one=__ONE__,two=__TWO__;
const caligoVoiceAccount='10001';
const root=fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()),'caligo-image-reuse-'));
const bytes=Buffer.from('__HEX__','hex'),md5=crypto.createHash('md5').update(bytes).digest('hex');
const dest=path.join(root,md5+'.png');
let listener,sends=0,reads=0;
const row=status=>({msgId:'12345',msgSeq:'1',msgRandom:'2',msgTime:'1794567890',
  guildId:'reused-cookie',chatType:1,msgType:2,subMsgType:0,peerUid:'u_peer',peerUin:'20002',
  senderUid:'u_self',senderUin:'10001',sendStatus:status,
  elements:[{elementType:2,elementId:'67890',picElement:{fileSize:String(bytes.length),
    md5HexStr:md5,picWidth:1,picHeight:1,picType:1000,picSubType:0}}]});
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
  assert.equal(JSON.parse(eval(poll)).sendResults['image-one'].status,'done');
  assert.equal(Object.hasOwn(globalThis.__caligo_res.sends,'image-one'),false);
  const second=JSON.parse(eval(two));
  listener.onAddSendMsg(row(1));listener.onMsgInfoListUpdate([row(2)]);await tick();await tick();
  const later=JSON.parse(eval(poll));
  assert.notEqual(later.sendResults['image-two']?.status,'done','old image notification must not confirm later send');
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
fn wrapper_image_cookie_history_survives_rearm_and_fails_closed() {
    use std::io::Write;
    let hex="89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63105432fe0f000214016604e0de7f0000000049454e44ae426082";
    let png: Vec<u8>=(0..hex.len()).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16).unwrap()).collect();
    let send=build_image_send_js(1,"10001","20002","image",1,5000,&png).unwrap();
    let fixture=r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs'),
 path=require('node:path'),os=require('node:os'),crypto=require('node:crypto'),send=__SEND__;
const bytes=Buffer.from('__HEX__','hex'),md5=crypto.createHash('md5').update(bytes).digest('hex');
const root=fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()),'caligo-image-cookie-'));
let cookie='first',calls=0;
const service={generateMsgUniqueId:()=>cookie,getRichMediaFilePathForGuild:()=>path.join(root,md5+'.png'),
 sendMsg(){calls++;return new Promise(()=>{});}};
const session={getMSFService:()=>({getServerTime:()=> 'time'}),
 getProfileService:()=>({getUidByUin:()=>new Map([['20002','u_peer']])})};
const resource=()=>({voiceAccount:'10001',stopped:false,sends:{},sendResults:{},ms:service,session});
function context(){const c={__caligo_res:resource(),process,Buffer,Promise,Date,Map,JSON,String,Object,Array,Number};vm.createContext(c);return c;}
const run=c=>JSON.parse(vm.runInContext(send,c));
try {
 const c=context();assert.equal(run(c).fired,true);assert.equal(calls,1);
 const descriptor=Object.getOwnPropertyDescriptor(c,'__caligo_image_cookie_guard_v1');
 assert.equal(descriptor.writable,false);assert.equal(descriptor.configurable,false);
 c.__caligo_res=resource();assert.deepEqual(run(c),{err:'invalid_cookie'});assert.equal(calls,1);
 for(let i=0;i<65535;i++) assert.equal(descriptor.value('filled-'+i),'ok');
 assert.equal(descriptor.value('first'),'invalid_cookie');assert.equal(descriptor.value('x'.repeat(257)),'invalid_cookie');
 cookie='new-cookie';assert.deepEqual(run(c),{err:'cookie_history_exhausted'});assert.equal(calls,1);
 for(const kind of ['null','mutable','accessor']) {
   const d=context();
   if(kind==='null') d.__caligo_image_cookie_guard_v1=null;
   else if(kind==='mutable') d.__caligo_image_cookie_guard_v1=()=> 'ok';
   else Object.defineProperty(d,'__caligo_image_cookie_guard_v1',{get(){throw Error('getter invoked');},configurable:true});
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
