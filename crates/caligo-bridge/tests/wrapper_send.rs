use caligo_bridge::qq_v8::{D8_POLL_JS, D8_START_JS, D8_STOP_JS, build_send_js, build_voice_send_js};

#[test]
fn wrapper_voice_send_requires_bound_account_and_exact_receipts() {
    use std::io::Write;
    let silk = b"\x02#!SILK_V3\x03\x00\x44\x55\x66";
    let quote = |text: &str| serde_json::to_string(text).unwrap();
    let send = build_voice_send_js(1, "10001", "20002", "voice-7", 42, 5000, silk).unwrap();
    let send_group = build_voice_send_js(2, "10001", "30003", "voice-7", 42, 5000, silk).unwrap();
    for malformed in [
        b"#!SILK_V3\x01\x00\x11".as_slice(),
        b"\x02#!SILK_V3".as_slice(),
        b"\x02#!SILK_V3\x00\x00".as_slice(),
        b"\x02#!SILK_V3\x02\x00\x11".as_slice(),
        b"\x02#!SILK_V3\xff\xff".as_slice(),
    ] {
        assert!(build_voice_send_js(1, "10001", "20002", "voice-7", 42, 5000, malformed).is_none());
    }
    let mut long = b"\x02#!SILK_V3".to_vec();
    long.extend_from_slice(&[1, 0, 0x11].repeat(51));
    assert!(build_voice_send_js(1, "10001", "20002", "voice-7", 42, 5000, &long).is_none());
    let script = format!(
        "const start={}, poll={}, stop={}, send={}, sendGroup={};\n",
        quote(D8_START_JS), quote(D8_POLL_JS), quote(D8_STOP_JS), quote(&send), quote(&send_group),
    ) + r#"
const assert = require('node:assert/strict');
const fs = require('node:fs');
const crypto = require('node:crypto');
const path = require('node:path');
const os = require('node:os');
const root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'caligo-voice-test-'));
const bytes = Buffer.from('02232153494c4b5f56330300445566','hex');
const md5 = crypto.createHash('md5').update(bytes).digest('hex');
const msgId = '90071992547409931237', elementId = '90071992547409931238';
const caligoVoiceAccount = '10001';
let listener, submits, reads, storageCalls, settle, mode, dest, now = 1000;
let chatType=1, peerUid='u_peer', peerUin='20002';
Date.now = () => now;
const flush = () => new Promise(resolve=>setImmediate(resolve));
function record(status, guildId='voice-cookie', fingerprint=md5, id=elementId) {
  return {msgId,msgSeq:'1',msgRandom:'2',msgTime:'1794567890',guildId,chatType,msgType:2,
    subMsgType:0,peerUid,peerUin,senderUid:'u_self',senderUin:'10001',
    sendStatus:status,elements:[{elementType:4,elementId:id,
      pttElement:{fileSize:String(bytes.length),md5HexStr:fingerprint,filePath:dest}}]};
}
const service = {
  addKernelMsgListener(value) { listener=value; return 8; },
  removeKernelMsgListener() { return true; },
  generateMsgUniqueId(chat,time) { assert.equal(chat,chatType); assert.equal(time,'qq-time'); return 'voice-cookie'; },
  getRichMediaFilePathForGuild(args) {
    storageCalls++;
    assert.deepEqual(args,{md5HexStr:md5,fileName:md5+'.silk',elementType:4,elementSubType:0,
      thumbSize:0,needCreate:true,downloadType:1,file_uuid:''});
    return dest;
  },
  sendMsg(id,peer,[element],options) {
    submits++;
    assert.equal(globalThis.__caligo_res.sends['voice-7'].wireSeq,42);
    assert.equal(id,'0'); assert.equal(options.size,0);
    assert.deepEqual(peer,{chatType,peerUid,guildId:'voice-cookie'});
    assert.equal(element.elementType,4);
    assert.equal(element.pttElement.md5HexStr,md5);
    assert.equal(element.pttElement.fileSize,String(bytes.length));
    assert.equal(element.pttElement.duration,1);
    assert.deepEqual(fs.readFileSync(dest),bytes);
    if (mode === 'throw') throw new Error('private cache path: '+dest);
    return new Promise(resolve=>{ settle=resolve; });
  },
  getMsgsByMsgId(peer,ids) {
    reads++;
    assert.deepEqual(ids,[msgId]);
    assert.equal(peer.guildId,'voice-cookie');
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
  mode=kind; submits=reads=storageCalls=0; settle=undefined; now=1000;
  dest=path.join(root,kind,md5+'.silk');
  const caligoVoiceAccount=armedAccount;
  assert.ok(eval(start).startsWith('ARMED'));
}
const run = () => JSON.parse(eval(send));
function outcome(projected=false) {
  const batch=JSON.parse(eval(poll));
  assert.equal(batch.items.length, projected ? 1 : 0);
  if (projected) {
    assert.equal(batch.items[0].source,'add_send');
    assert.deepEqual(batch.items[0].record.elements,[{elementType:4,voice:{element_id:elementId,
      size:bytes.length,md5}}]);
  }
  assert.ok(!JSON.stringify(batch).includes('filePath'));
  return batch.sendResults['voice-7'];
}
function notify() {
  listener.onAddSendMsg(record(1,'wrong-cookie'));
  listener.onAddSendMsg(record(1,'voice-cookie','0'.repeat(32)));
  listener.onAddSendMsg(record(1));
  listener.onMsgInfoListUpdate([record(2,'voice-cookie',md5,'999'),record(2)]);
}
(async()=>{
  setup('unbound','30003');
  assert.deepEqual(run(),{err:'voice_profile_unavailable'});
  assert.equal(storageCalls+submits,0);
  setup('conflict');
  fs.mkdirSync(path.dirname(dest),{recursive:true});
  const wrong=Buffer.alloc(bytes.length);
  fs.writeFileSync(dest,wrong);
  assert.deepEqual(run(),{err:'voice_storage_mismatch'});
  assert.deepEqual(fs.readFileSync(dest),wrong);
  assert.equal(submits,0);
  setup('bad_path');
  dest=path.join(root,'bad_path','other.silk');
  assert.deepEqual(run(),{err:'invalid_voice_storage_path'});
  assert.equal(submits,0);
  assert.equal(fs.existsSync(dest),false);

  setup('good');
  assert.deepEqual(run(),{fired:true,mid:'voice-7'});
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
  assert.deepEqual(outcome(),{status:'unknown',reason:'voice_submit_unknown'});
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
        .spawn()
        .unwrap();
    node.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let output = node.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn wrapper_send_uses_production_listener_and_exact_readback_before_success() {
    let send = build_send_js(1, "20002", "hello", "10001", "core-request-7", 42, 5_000);
    let fixture = format!(
        r#"
const assert = require('node:assert/strict');
const vm = require('node:vm');
const start = {start};
const poll = {poll};
const send = {send};
const calls = [];
let listener;
const record = (id, status, guildId='different-on-readback') => ({{
  msgId:id, msgSeq:'1', msgRandom:'2', msgTime:'1794567890', guildId,
  chatType:1, msgType:2, subMsgType:0, peerUid:'u_peer', peerUin:'20002',
  senderUid:'u_self', senderUin:'10001', sendStatus:status,
  elements:[{{elementType:1,textElement:{{content:'hello',atType:0}}}}]
}});
const service = {{
  generateMsgUniqueId(chat, time) {{ calls.push(['cookie', chat, time]); return 'opaque-local-cookie'; }},
  addKernelMsgListener(value) {{ listener=value; return 8; }},
  removeKernelMsgListener() {{ return true; }},
  sendMsg(id, peer, elements) {{
    assert.equal(context.__caligo_res.sends['core-request-7'].wireSeq, 42);
    assert.equal(peer.chatType,1); assert.equal(peer.guildId,'opaque-local-cookie'); assert.equal(peer.peerUid,'u_peer');
    calls.push(['send',id]);
    queueMicrotask(() => {{
      listener.onAddSendMsg(record('native-99',1,'opaque-local-cookie'));
      listener.onMsgInfoListUpdate([record('native-99',2,'opaque-local-cookie')]);
    }});
    return Promise.resolve({{result:0,msgId:'must-not-trust-promise-id'}});
  }},
  getMsgsByMsgId(peer, ids) {{
    calls.push(['query', peer, ids]);
    return Promise.resolve({{result:0,msgList:[record('native-99',2)]}});
  }}
}};
const session = {{getMSFService: () => ({{getServerTime: () => 'qq-server-time'}}), getSessionId: () => 'nt_0', getMsgService: () => service,
  getProfileService: () => ({{getUidByUin(source, keys) {{ assert.equal(source,'FriendsServiceImpl'); assert.equal(keys.length,1); assert.equal(keys[0],'20002'); return new Map([['20002','u_peer']]); }}}})}};
const context = {{Buffer,Promise,Date,Map,JSON,String,Object,Array,Number,process:{{_linkedBinding:()=>({{NodeIQQNTWrapperSession:{{getNTWrapperSession:()=>session}}}})}}}};
context.session=session; context.service=service;
vm.createContext(context);
const armed=vm.runInContext(start,context);
assert.ok(armed.startsWith('ARMED'), armed);
const fired=JSON.parse(vm.runInContext(send,context));
assert.deepEqual(fired,{{fired:true,mid:'core-request-7'}});
(async()=>{{
  for (let i=0; i<5 && context.__caligo_res.sendResults['core-request-7'].status !== 'done'; i++)
    await new Promise(resolve=>setImmediate(resolve));
  const batch=JSON.parse(vm.runInContext(poll,context));
  assert.equal(batch.sid,'nt_0');
  assert.deepEqual(batch.textSendTrace,[]);
  assert.equal(calls.filter(c=>c[0]==='query').length,1, JSON.stringify({{calls,batch}}));
  assert.equal(batch.sendResults['core-request-7'].status,'done');
  assert.equal(batch.sendResults['core-request-7'].msgId,'native-99');
  assert.equal(calls.find(c=>c[0]==='query')[2][0],'native-99');
  assert.equal(calls.find(c=>c[0]==='query')[1].guildId,'opaque-local-cookie');
  assert.notEqual(batch.sendResults['core-request-7'].msgId,'opaque-local-cookie');
  assert.ok(calls.findIndex(c=>c[0]==='send')>=0);
  assert.ok(calls.findIndex(c=>c[0]==='query')>calls.findIndex(c=>c[0]==='send'));
  assert.ok(!send.includes('res.msgId'));
}})().catch(error=>{{console.error(error);process.exitCode=1;}});
"#,
        start = serde_json::to_string(D8_START_JS).unwrap(),
        poll = serde_json::to_string(D8_POLL_JS).unwrap(),
        send = serde_json::to_string(&send).unwrap(),
    );
    let output = std::process::Command::new("node")
        .args(["--unhandled-rejections=strict", "-e", &fixture])
        .output()
        .expect("Node.js is required for the wrapper send fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn wrapper_text_send_uncertain_submission_stays_unknown() {
    let send = build_send_js(1, "20002", "hello", "10001", "request", 42, 5_000);
    let fixture = format!(r#"
const assert = require('node:assert/strict');
const vm = require('node:vm');
const send = {send};
async function run(mode) {{
  const sends = {{}};
  const service = {{generateMsgUniqueId:()=> 'cookie', sendMsg() {{
    assert.ok(sends.request, 'registration must precede submission');
    if (mode === 'throw') throw Error('submit uncertain');
    if (mode === 'reject') return Promise.reject(Error('submit uncertain'));
    if (mode === 'code') return Promise.resolve({{result: 37}});
    if (mode === 'ambiguous-code') {{ sends.request.addId='native-candidate'; return Promise.resolve({{result:37}}); }}
    if (mode === 'ambiguous-readback') {{ sends.request.addId='native-candidate'; sends.request.readback={{status:'readback',msgId:'native-candidate'}}; return Promise.resolve({{result:37}}); }}
    if (mode === 'accepted') return Promise.resolve({{result: 0}});
    return Promise.resolve({{result: undefined}});
  }}}};
  const session = {{getMSFService:()=>({{getServerTime:()=> 'time'}}),
    getProfileService:()=>({{getUidByUin:()=>new Map([['20002','u_peer']])}})}};
  const R = {{sends, sendResults:{{}}, ms:service, session}};
  const context = {{__caligo_res:R, Promise, Date, Map, JSON, String, Object, Array, Number}};
  vm.createContext(context);
  assert.equal(JSON.parse(vm.runInContext(send,context)).fired,true);
  await new Promise(resolve=>setImmediate(resolve));
  if (mode === 'code') {{ assert.equal(R.sendResults.request.status,'unknown'); assert.equal(R.sendResults.request.reason,'send_result_ambiguous=37'); }}
  else if (mode === 'ambiguous-code' || mode === 'ambiguous-readback') {{ assert.equal(R.sendResults.request.status,'unknown'); assert.equal(R.sendResults.request.reason,'send_result_ambiguous=37'); }}
  else assert.equal(R.sendResults.request.status,mode === 'accepted' ? 'accepted' : 'unknown',mode);
}}
async function late(mode) {{
  let finish;
  const service={{generateMsgUniqueId:()=> 'cookie',sendMsg:()=>new Promise(resolve=>{{finish=resolve;}})}};
  const session={{getMSFService:()=>({{getServerTime:()=> 'time'}}),
    getProfileService:()=>({{getUidByUin:()=>new Map([['20002','u_peer']])}})}};
  const R={{sends:{{}},sendResults:{{}},ms:service,session}};
  const context={{__caligo_res:R,Promise,Date,Map,JSON,String,Object,Array,Number}};
  vm.createContext(context);
  assert.equal(JSON.parse(vm.runInContext(send,context)).fired,true);
  if (mode === 'timeout') {{ R.sends.request.timedOut=true; R.sendResults.request={{status:'unknown',reason:'timeout'}}; }}
  else if (mode === 'stopped') R.stopped=true;
  else {{ R.sends.request={{replacement:true}}; R.sendResults.request={{status:'pending'}}; }}
  finish({{result:37}});
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(R.sendResults.request.status, mode === 'timeout' ? 'unknown' : 'pending', mode);
}}
(async()=>{{ for (const mode of ['throw','reject','malformed','code','ambiguous-code','ambiguous-readback','accepted']) await run(mode);
  for (const mode of ['timeout','stopped','replaced']) await late(mode); }})()
  .catch(error=>{{console.error(error);process.exitCode=1;}});
"#, send = serde_json::to_string(&send).unwrap());
    let output = std::process::Command::new("node")
        .args(["--unhandled-rejections=strict", "-e", &fixture])
        .output()
        .expect("Node.js is required for the wrapper send fixture");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn wrapper_text_inline_duplicate_and_old_generation_callbacks_are_scoped() {
    let send = build_send_js(1, "20002", "hello", "10001", "request", 42, 5_000);
    let fixture = format!(r#"
const assert=require('node:assert/strict'), vm=require('node:vm');
const start={start}, stop={stop}, poll={poll}, send={send};
let listener, count=0, queries=0, mode='inline', complete;
const record=(id,status,guild='cookie')=>({{msgId:id,msgSeq:'1',msgRandom:'2',msgTime:'1794567890',guildId:guild,
  chatType:1,msgType:2,subMsgType:0,peerUid:'u_peer',peerUin:'20002',senderUid:'u_self',senderUin:'10001',sendStatus:status,
  elements:[{{elementType:1,textElement:{{content:'hello',atType:0}}}}]}});
const service={{generateMsgUniqueId:()=> 'cookie',addKernelMsgListener(l){{listener=l;return 7;}},removeKernelMsgListener(){{return true;}},
  sendMsg(){{count++; assert.ok(context.__caligo_res.sends.request);
    if(mode==='inline') {{ listener.onAddSendMsg(record('native-1',1)); listener.onAddSendMsg(record('native-1',1));
      listener.onMsgInfoListUpdate([record('native-1',2),record('native-1',2)]); return Promise.resolve({{result:0}}); }}
    return new Promise(resolve=>{{complete=resolve;}});
  }},getMsgsByMsgId(){{queries++;return Promise.resolve({{result:0,msgList:[record('native-1',2,'other')]}});}}}};
const session={{getSessionId:()=> 'nt_0',getMsgService:()=>service,getMSFService:()=>({{getServerTime:()=> 'time'}}),
  getProfileService:()=>({{getUidByUin:()=>new Map([['20002','u_peer']])}})}};
const context={{Buffer,Promise,Date,Map,JSON,String,Object,Array,Number,caligoSendTrace:true,process:{{_linkedBinding:()=>({{NodeIQQNTWrapperSession:{{getNTWrapperSession:()=>session}}}})}}}};
vm.createContext(context);
const run=s=>vm.runInContext(s,context);
(async()=>{{
  assert.match(run(start),/^ARMED/); assert.equal(JSON.parse(run(send)).fired,true);
  for(let i=0;i<5;i++) await new Promise(resolve=>setImmediate(resolve));
  let batch=JSON.parse(run(poll)); assert.equal(batch.sendResults.request.status,'done');
  assert.equal(batch.sendResults.request.msgId,'native-1'); assert.equal(queries,1);
  assert.ok(batch.textSendTrace.indexOf('registered') < batch.textSendTrace.indexOf('submit_enter'));
  assert.ok(batch.textSendTrace.indexOf('matched_add') < batch.textSendTrace.indexOf('submit_return'));
  assert.ok(batch.textSendTrace.includes('promise_accepted') && batch.textSendTrace.includes('readback_matched') && batch.textSendTrace.includes('settled'));
  listener.onAddSendMsg(record('native-1',1)); listener.onMsgInfoListUpdate([record('native-1',2)]);
  assert.equal(JSON.parse(run(poll)).sendResults.request,undefined);
  mode='pending'; assert.equal(JSON.parse(run(send)).fired,true); let old=listener, oldComplete=complete, oldState=context.__caligo_res;
  let staleDownloads=0; oldState.voiceDownload=()=>{{ staleDownloads++; }};
  const oldSnapshot=JSON.stringify({{recv:oldState.recv,bytes:oldState.recvBytes,rejected:oldState.rejected,overflow:oldState.overflow,images:oldState.imageShapes}});
  service.removeKernelMsgListener=()=>{{ old.onRecvMsg([record('inline-stop',2)]); old.onRichMediaDownloadComplete({{status:0}}); return true; }};
  assert.match(run(stop),/^STOPPED/);
  assert.match(run(start),/^ARMED/);
  assert.equal(JSON.parse(run(send)).fired,true); const current=context.__caligo_res;
  old.onRecvMsg([record('late-recv',2)]); old.onRecvMsg(null);
  old.onAddSendMsg(record('old-native',1)); old.onMsgInfoListUpdate([record('old-native',2)]);
  old.onRichMediaDownloadComplete({{status:0}});
  oldComplete({{result:37}}); await new Promise(resolve=>setImmediate(resolve));
  assert.equal(JSON.stringify({{recv:oldState.recv,bytes:oldState.recvBytes,rejected:oldState.rejected,overflow:oldState.overflow,images:oldState.imageShapes}}),oldSnapshot);
  assert.equal(staleDownloads,2);
  assert.equal(current.sendResults.request.status,'pending'); assert.equal(current.sends.request.addId,null);
  assert.equal(count,3); assert.equal(queries,1);
  service.removeKernelMsgListener=()=>{{throw Error('not removed');}};
  assert.equal(run(stop),'ERR:listener_remove_failed');
  assert.equal(context.__caligo_res,current); assert.equal(current.stopped,true);
  let failedDownloads=0; current.voiceDownload=()=>{{ failedDownloads++; }};
  const failedSnapshot=JSON.stringify({{recv:current.recv,bytes:current.recvBytes,rejected:current.rejected}});
  listener.onRecvMsg([record('failed-stop',2)]); listener.onAddSendMsg(record('failed-stop',1));
  listener.onMsgInfoListUpdate(null); listener.onRichMediaDownloadComplete({{status:0}});
  assert.equal(JSON.stringify({{recv:current.recv,bytes:current.recvBytes,rejected:current.rejected}}),failedSnapshot);
  assert.equal(failedDownloads,1);
  assert.equal(run(start),'ERR:stopped_listener_retained');
}})().catch(e=>{{console.error(e);process.exitCode=1;}});
"#,start=serde_json::to_string(D8_START_JS).unwrap(),stop=serde_json::to_string(D8_STOP_JS).unwrap(),
        poll=serde_json::to_string(D8_POLL_JS).unwrap(),send=serde_json::to_string(&send).unwrap());
    let output=std::process::Command::new("node").args(["--unhandled-rejections=strict","-e",&fixture])
        .output().expect("Node.js is required for the wrapper send fixture");
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
}
