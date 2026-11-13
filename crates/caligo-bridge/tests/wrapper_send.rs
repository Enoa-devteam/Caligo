use caligo_bridge::qq_v8::{D8_POLL_JS, D8_START_JS, D8_STOP_JS, build_send_js, build_voice_send_js};

#[test]
fn wrapper_voice_send_requires_bound_account_and_exact_receipts() {
    use std::io::Write;
    let silk = b"\x02#!SILK_V3\x03\x00\x44\x55\x66";
    let quote = |text: &str| serde_json::to_string(text).unwrap();
    let send = build_voice_send_js("10001", "20002", "voice-7", 42, 5000, silk).unwrap();
    for malformed in [
        b"#!SILK_V3\x01\x00\x11".as_slice(),
        b"\x02#!SILK_V3".as_slice(),
        b"\x02#!SILK_V3\x00\x00".as_slice(),
        b"\x02#!SILK_V3\x02\x00\x11".as_slice(),
        b"\x02#!SILK_V3\xff\xff".as_slice(),
    ] {
        assert!(build_voice_send_js("10001", "20002", "voice-7", 42, 5000, malformed).is_none());
    }
    let mut long = b"\x02#!SILK_V3".to_vec();
    long.extend_from_slice(&[1, 0, 0x11].repeat(51));
    assert!(build_voice_send_js("10001", "20002", "voice-7", 42, 5000, &long).is_none());
    let script = format!(
        "const start={}, poll={}, stop={}, send={};\n",
        quote(D8_START_JS), quote(D8_POLL_JS), quote(D8_STOP_JS), quote(&send),
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
Date.now = () => now;
const flush = () => new Promise(resolve=>setImmediate(resolve));
function record(status, guildId='voice-cookie', fingerprint=md5, id=elementId) {
  return {msgId,msgSeq:'1',msgRandom:'2',msgTime:'1794567890',guildId,chatType:1,msgType:2,
    subMsgType:0,peerUid:'u_peer',peerUin:'20002',senderUid:'u_self',senderUin:'10001',
    sendStatus:status,elements:[{elementType:4,elementId:id,
      pttElement:{fileSize:String(bytes.length),md5HexStr:fingerprint,filePath:dest}}]};
}
const service = {
  addKernelMsgListener(value) { listener=value; return 8; },
  removeKernelMsgListener() { return true; },
  generateMsgUniqueId(chat,time) { assert.equal(chat,1); assert.equal(time,'qq-time'); return 'voice-cookie'; },
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
    assert.deepEqual(peer,{chatType:1,peerUid:'u_peer',guildId:'voice-cookie'});
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
function outcome() {
  const batch=JSON.parse(eval(poll));
  assert.equal(batch.items.length,0);
  assert.ok(!JSON.stringify(batch).includes(md5));
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
  assert.deepEqual(outcome(),{status:'done',msgId});
  assert.equal(submits,1); assert.equal(reads,1);
  assert.deepEqual(fs.readFileSync(dest),bytes);

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
