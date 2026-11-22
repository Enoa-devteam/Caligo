#![cfg(feature = "research")]

use caligo_bridge::qq_v8::D8_SURFACE_PROBE_JS;
use std::io::Write as _;
use std::process::{Command, Stdio};

/// The probe must expose only bounded property names and typeof values: no
/// message data, no identity strings, no function sources and no error text.
#[test]
fn surface_probe_reports_names_and_types_only() {
    let probe = serde_json::to_string(D8_SURFACE_PROBE_JS).expect("probe literal");
    let fixture = format!(
        r#"
const vm = require('node:vm');
const assert = require('node:assert/strict');
const probe = {probe};
let activeSession = null;
const context = vm.createContext({{
  __caligo_d8: {{}},
  process: {{_linkedBinding: () => ({{
    NodeIQQNTWrapperSession: {{getNTWrapperSession: () => activeSession}},
    NodeIQQNTWrapperNetwork: function FakeNetwork() {{}},
    NodeIKernelLoginService: function FakeLogin() {{}}
  }})}},
  Buffer
}});
function run(session) {{
  activeSession = session;
  return vm.runInContext(probe, context);
}}
const login = {{addKernelLoginListener: () => 1, removeKernelLoginListener: () => 1}};
const session = {{
  getLoginService: () => login,
  getNetworkService: undefined,
  getProfileService: () => ({{getSelfStatus: () => Promise.resolve(2), getStatusInfo: () => 0, getCoreInfo: () => 0}}),
  getMsgService: () => ({{sendMsg: () => 0, getOnLineDev: () => 0}}),
  secret: 'PRIVATE_SHOULD_NOT_LEAK'
}};
(async () => {{
  const out = JSON.parse(run(session));
  assert.ok(out.session.includes('getLoginService'));
  assert.equal(out.services.getLoginService.type, 'object');
  assert.deepEqual(out.services.getLoginService.keys.slice().sort(),
    ['addKernelLoginListener','removeKernelLoginListener']);
  assert.equal(out.services.getNetworkService.type, 'undefined');
  assert.equal(out.binding_types.NodeIQQNTWrapperNetwork, 'function');
  assert.equal(out.binding_classes.NodeIQQNTWrapperNetwork.type, 'function');
  assert.deepEqual(out.status_probe.self_status, {{type:'promise'}});
  assert.deepEqual(out.status_probe.status_info, {{type:'function', arity:0}});
  assert.deepEqual(out.status_probe.online_dev, {{type:'function', arity:0}});
  assert.equal(JSON.stringify(out).includes('PRIVATE_SHOULD_NOT_LEAK'), false);
  await new Promise(resolve => setImmediate(resolve));
  const second = JSON.parse(run(session));
  assert.deepEqual(second.self_status_result, {{type:'number', value:2}});
  const missing = JSON.parse(run(null));
  assert.equal(missing.error, 'no_session');
  const throwing = {{getLoginService: () => {{ throw new Error('PRIVATE_ERROR_TEXT'); }}}};
  const broken = JSON.parse(run(throwing));
  assert.equal(broken.services.getLoginService.error, 'Error');
  assert.equal(JSON.stringify(broken).includes('PRIVATE_ERROR_TEXT'), false);
  console.log('SURFACE_PROBE_OK');
}})();
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("SURFACE_PROBE_OK"));
}
