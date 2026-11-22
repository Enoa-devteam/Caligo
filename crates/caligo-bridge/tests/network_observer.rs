#![cfg(feature = "research")]

use caligo_bridge::qq_v8::{D8_NETWORK_EVENTS_JS, D8_NETWORK_OBSERVER_STATUS_JS, D8_START_JS, D8_STOP_JS};
use std::io::Write as _;
use std::process::{Command, Stdio};

/// The network observer records only bounded argument shapes (type, number,
/// string length, object keys). It must never leak string content, and after
/// STOP the callbacks must stop appending rows.
#[test]
fn network_observer_captures_bounded_shapes_only() {
    let scripts = serde_json::json!({
        "start": D8_START_JS,
        "stop": D8_STOP_JS,
        "drain": D8_NETWORK_EVENTS_JS,
        "status": D8_NETWORK_OBSERVER_STATUS_JS,
    });
    let fixture = format!(
        r#"
const vm = require('node:vm');
const assert = require('node:assert/strict');
const scripts = {scripts};
const callbacks = {{}};
const listeners = [];
const ms = {{
  addKernelMsgListener(listener) {{ listeners.push(listener); return 15; }},
  removeKernelMsgListener(id) {{ assert.equal(id, 15); return 'ok'; }},
  sendMsg() {{ throw new Error('send is not part of this fixture'); }},
}};
const session = {{
  getSessionId: () => 1,
  getMsgService: () => ms,
  setOnNetworkChanged(cb) {{ callbacks.network = cb; }},
  setOnMsfStatusChanged(cb) {{ callbacks.msf = cb; }},
  setOnWeakNetChanged(cb) {{ callbacks.weak = cb; }},
}};
const networkService = {{
  addNetworkServiceListener(cb) {{ callbacks.netservice = cb; }},
  getNetworkStatus() {{ return 1; }},
}};
const context = vm.createContext({{
  process: {{_linkedBinding: () => ({{
    NodeIQQNTWrapperSession: {{getNTWrapperSession: () => session}},
    NodeIQQNTWrapperNetwork: {{openNetworkService: () => networkService}}
  }})}},
  caligoNetworkObserverReady: true,
  caligoSendTrace: false,
  Buffer,
}});
const run = script => vm.runInContext(script, context);
assert.equal(run(scripts.start), 'ARMED sid=nt_0 lidRet=15');
assert.equal(run(scripts.status), 'addNetworkServiceListener,setOnMsfStatusChanged,setOnNetworkChanged,setOnWeakNetChanged');
assert.equal(run(scripts.drain), 'NONE');
callbacks.network({{code:2, nested:{{a:1}}}}, 'PRIVATE_TEXT_SHOULD_NOT_LEAK', true);
callbacks.msf('PRIVATE_MSF_TEXT', 7);
const batch = JSON.parse(run(scripts.drain));
assert.equal(batch.events.length, 2);
assert.deepEqual(batch.events[0].args[0], {{type:'object', keys:['code','nested']}});
assert.deepEqual(batch.events[0].args[1], {{type:'string', length:28}});
assert.deepEqual(batch.events[0].args[2], {{type:'boolean', value:true}});
assert.deepEqual(batch.events[1].args, [{{type:'string', length:16}}, {{type:'number', value:7}}]);
assert.equal(JSON.stringify(batch).includes('PRIVATE'), false);
assert.equal(run(scripts.drain), 'NONE');
callbacks.netservice({{status:3}});
const netBatch = JSON.parse(run(scripts.drain));
assert.equal(netBatch.events.length, 1);
assert.deepEqual(netBatch.events[0].args[0], {{type:'object', keys:['status']}});
assert.equal(run(scripts.stop), 'STOPPED sid=nt_0 remove=ok');
callbacks.network(1);
assert.equal(run(scripts.drain), 'NONE');
console.log('NETWORK_OBSERVER_OK');
"#
    );
    let mut child = Command::new("node")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node available");
    child.stdin.take().expect("stdin").write_all(fixture.as_bytes()).expect("write fixture");
    let output = child.wait_with_output().expect("node exit");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("NETWORK_OBSERVER_OK"));
}
