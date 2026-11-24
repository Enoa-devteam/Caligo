use caligo_bridge::qq_v8::{build_bound_poll_js,poll_listener_bound_json};
use caligo_model::wrapper_message::{decode_d8_batch,WrapperBinding};
use std::io::Write;
#[test]
fn mismatched_bound_session_preserves_results_and_receive_state_until_matching_poll() {
 assert!(build_bound_poll_js("").is_none());
 assert_eq!(poll_listener_bound_json("").unwrap(),"NOT_ARMED:session_unavailable");
 assert!(build_bound_poll_js(&"x".repeat(129)).is_none());
 let good=build_bound_poll_js("nt_1").unwrap();
 let bad=build_bound_poll_js("nt_\"other").unwrap();
 let js=r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),bad=__BAD__,good=__GOOD__;
const R={sid:'nt_1',sends:{stale:{deadline:0,done:false,timedOut:false}},sendResults:{r1:{status:'done',msgId:'123'}},recv:[{invalid:'fixture'}],recvBytes:32,rejected:1,overflow:2};
const c={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
assert.equal(vm.runInContext(bad,c),'ERR:session_mismatch');
assert.equal(R.sendResults.r1.msgId,'123');assert.equal(R.recv.length,1);assert.equal(R.recvBytes,32);
assert.equal(R.rejected,1);assert.equal(R.overflow,2);assert.equal(R.sends.stale.timedOut,false);
process.stdout.write(vm.runInContext(good,c));
"#.replace("__BAD__",&serde_json::to_string(&bad).unwrap()).replace("__GOOD__",&serde_json::to_string(&good).unwrap());
 let mut child=std::process::Command::new("node").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
 child.stdin.take().unwrap().write_all(js.as_bytes()).unwrap();let out=child.wait_with_output().unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
 let binding=WrapperBinding{account:"10001".into(),session_generation:1,host_nonce:1,wrapper_session:"nt_1".into()};
 let batch=decode_d8_batch(std::str::from_utf8(&out.stdout).unwrap(),&binding).unwrap();
 assert_eq!(batch.send_results["r1"]["msgId"],"123");assert_eq!(batch.stats.rejected,2);assert_eq!(batch.stats.overflow,2);
}
