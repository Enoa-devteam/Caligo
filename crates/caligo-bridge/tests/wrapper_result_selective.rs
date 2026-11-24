use caligo_bridge::qq_v8::{build_bound_poll_js,build_poll_result_ack_js,PollResultReceipt};
use std::io::Write;
#[test]
fn acknowledgement_must_not_clear_unmatched_results_in_same_batch() {
 let poll=build_bound_poll_js("nt_1").unwrap();
 let receipt=PollResultReceipt{generation:"55".repeat(16),sequence:1};
 let ack=build_poll_result_ack_js("nt_1",&receipt,&["matched".into()]).unwrap();
 let forged=build_poll_result_ack_js("nt_1",&receipt,&["matched".into(),"absent".into()]).unwrap();
 let different=build_poll_result_ack_js("nt_1",&receipt,&["orphan".into()]).unwrap();
 assert!(build_poll_result_ack_js("nt_1",&receipt,&["matched".into(),"matched".into()]).is_none());
 let too_many:Vec<String>=(0..129).map(|n|format!("key-{n}")).collect();
 assert!(build_poll_result_ack_js("nt_1",&receipt,&too_many).is_none());
 assert!(build_poll_result_ack_js("nt_1",&receipt,&["x".repeat(524288)]).is_none());
 let empty=build_poll_result_ack_js("nt_1",&PollResultReceipt{generation:"55".repeat(16),sequence:2},&[]).unwrap();
 let js=r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__,ack=__ACK__,forged=__FORGED__,different=__DIFFERENT__,empty=__EMPTY__;
const R={sid:'nt_1',sends:{},sendResults:{matched:{status:'done',msgId:'123'},orphan:{status:'unknown',reason:'timeout'}},recv:[],recvBytes:0,rejected:0,overflow:0};
const process={getBuiltinModule:()=>({randomBytes:()=>Buffer.from('55'.repeat(16),'hex')})};
const c={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
const batch=JSON.parse(vm.runInContext(poll,c));assert.equal(Object.keys(batch.sendResults).length,2);
assert.equal(vm.runInContext(forged,c),'ERR:receipt_keys');assert.ok(R.sendResults.matched);
assert.equal(vm.runInContext(ack,c),'ACKED');assert.equal(R.sendResults.matched,undefined);
assert.equal(vm.runInContext(ack,c),'ACKED');assert.equal(vm.runInContext(different,c),'ERR:receipt_replay_mismatch');
assert.equal(R.sendResults.orphan.status,'unknown','unmatched result must remain available');
JSON.parse(vm.runInContext(poll,c));assert.equal(vm.runInContext(empty,c),'ACKED');assert.equal(R.sendResults.orphan.status,'unknown');
"#.replace("__POLL__",&serde_json::to_string(&poll).unwrap()).replace("__ACK__",&serde_json::to_string(&ack).unwrap()).replace("__FORGED__",&serde_json::to_string(&forged).unwrap()).replace("__DIFFERENT__",&serde_json::to_string(&different).unwrap()).replace("__EMPTY__",&serde_json::to_string(&empty).unwrap());
 let mut child=std::process::Command::new("node").stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
 child.stdin.take().unwrap().write_all(js.as_bytes()).unwrap();let out=child.wait_with_output().unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
}
