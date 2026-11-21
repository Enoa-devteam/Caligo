#![cfg(feature = "research")]

use caligo_bridge::qq_v8::D8_SURFACE_PROBE_JS;
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
function run(session) {{
  const context = vm.createContext({{
    process: {{_linkedBinding: () => ({{NodeIQQNTWrapperSession: {{getNTWrapperSession: () => session}}}})}},
    Buffer
  }});
  return vm.runInContext(probe, context);
}}
const login = {{addKernelLoginListener: () => 1, removeKernelLoginListener: () => 1}};
const session = {{
  getLoginService: () => login,
  getNetworkService: undefined,
  getProfileService: () => ({{}}),
  getMsgService: () => ({{sendMsg: () => 0}}),
  secret: 'PRIVATE_SHOULD_NOT_LEAK'
}};
const out = JSON.parse(run(session));
assert.ok(out.session.includes('getLoginService'));
assert.equal(out.services.getLoginService.type, 'object');
assert.deepEqual(out.services.getLoginService.keys.slice().sort(),
  ['addKernelLoginListener','removeKernelLoginListener']);
assert.equal(out.services.getNetworkService.type, 'undefined');
assert.equal(JSON.stringify(out).includes('PRIVATE_SHOULD_NOT_LEAK'), false);
const missing = JSON.parse(run(null));
assert.equal(missing.error, 'no_session');
const throwing = {{getLoginService: () => {{ throw new Error('PRIVATE_ERROR_TEXT'); }}}};
const broken = JSON.parse(run(throwing));
assert.equal(broken.services.getLoginService.error, 'Error');
assert.equal(JSON.stringify(broken).includes('PRIVATE_ERROR_TEXT'), false);
console.log('SURFACE_PROBE_OK');
"#
    );
    let output = Command::new("node")
        .arg("-e")
        .arg(&fixture)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("node available");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("SURFACE_PROBE_OK"));
}
