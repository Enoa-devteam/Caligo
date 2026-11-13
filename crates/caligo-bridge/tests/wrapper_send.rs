use caligo_bridge::qq_v8::{D8_POLL_JS, D8_START_JS, build_send_js};

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
