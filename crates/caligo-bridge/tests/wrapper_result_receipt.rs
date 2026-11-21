use caligo_bridge::qq_v8::{build_bound_poll_js,build_poll_result_ack_js,poll_result_receipt,advance_poll_result_ack,PollResultReceipt,PollResultAck};
use std::io::Write;
#[test]
fn bound_poll_replays_terminal_result_after_lost_v8_return() {
 let poll=build_bound_poll_js("nt_1").unwrap();
 let js=r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__;
const R={sid:'nt_1',sends:{},sendResults:{r1:{status:'done',msgId:'123'},waiting:{status:'pending'}},recv:[],recvBytes:0,rejected:0,overflow:0};
const c={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
const first=JSON.parse(vm.runInContext(poll,c));assert.equal(first.sendResults.r1.msgId,'123');
assert.equal(R.sendResults.r1.msgId,'123','terminal must remain until receipt is acknowledged');
const second=JSON.parse(vm.runInContext(poll,c));assert.equal(second.sendResults.r1.msgId,'123');
assert.ok(first.resultReceipt && second.resultReceipt);assert.notDeepEqual(first.resultReceipt,second.resultReceipt);
"#.replace("__POLL__",&serde_json::to_string(&poll).unwrap());
 let mut child=std::process::Command::new("node").stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
 child.stdin.take().unwrap().write_all(js.as_bytes()).unwrap();let out=child.wait_with_output().unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
}

#[test]
fn receipt_ack_is_idempotent_scoped_and_never_deletes_newer_results() {
 let poll=build_bound_poll_js("nt_1").unwrap();
 let receipt=|n| PollResultReceipt{generation:"11".repeat(16),sequence:n};
 let ack1=build_poll_result_ack_js("nt_1",&receipt(1),&["r1".into(),"r2".into()]).unwrap();
 let ack2=build_poll_result_ack_js("nt_1",&receipt(2),&["r1".into()]).unwrap();
 let wrong=build_poll_result_ack_js("nt_other",&receipt(1),&["r1".into(),"r2".into()]).unwrap();
 assert!(build_poll_result_ack_js("nt_1",&receipt(0),&[]).is_none());
 assert!(poll_result_receipt(r#"{"resultReceipt":{"generation":"bad","sequence":1}}"#).is_err());
 assert!(poll_result_receipt(r#"{"resultReceipt":{"generation":"11111111111111111111111111111111","sequence":0}}"#).is_err());
 assert!(poll_result_receipt(r#"{}"#).unwrap().is_none());
 let js=r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__,ack1=__ACK1__,ack2=__ACK2__,wrong=__WRONG__;
let n=0;const fakeProcess={getBuiltinModule:name=>({randomBytes:()=>Buffer.from((++n===1?'11':'22').repeat(16),'hex')})};
const make=()=>({sid:'nt_1',sends:{},sendResults:{r1:{status:'unknown',reason:'timeout'},r2:{status:'done',msgId:'222'},p:{status:'pending'}},recv:[],recvBytes:0,rejected:0,overflow:0});
let R=make();const c={__caligo_res:R,process:fakeProcess,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
const run=s=>vm.runInContext(s,c);const first=JSON.parse(run(poll));assert.equal(first.resultReceipt.sequence,1);
assert.equal(run(wrong),'ERR:receipt_session');assert.equal(R.sendResults.r2.msgId,'222');
R.sendResults.r1={status:'done',msgId:'111'};assert.equal(run(ack1),'ACKED');
assert.equal(R.sendResults.r1.msgId,'111');assert.equal(R.sendResults.r2,undefined);assert.equal(R.sendResults.p.status,'pending');
const second=JSON.parse(run(poll));assert.equal(second.resultReceipt.sequence,2);
assert.equal(run(ack1),'ACKED');assert.equal(R.sendResults.r1.msgId,'111');
assert.equal(run(ack2),'ACKED');assert.equal(run(ack2),'ACKED');assert.equal(R.sendResults.r1,undefined);
R=make();c.__caligo_res=R;const fresh=JSON.parse(run(poll));assert.notEqual(fresh.resultReceipt.generation,second.resultReceipt.generation);
assert.equal(run(ack2),'ERR:receipt_stale');assert.equal(R.sendResults.r1.status,'unknown');
"#.replace("__POLL__",&serde_json::to_string(&poll).unwrap()).replace("__ACK1__",&serde_json::to_string(&ack1).unwrap())
 .replace("__ACK2__",&serde_json::to_string(&ack2).unwrap()).replace("__WRONG__",&serde_json::to_string(&wrong).unwrap());
 let mut child=std::process::Command::new("node").stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
 child.stdin.take().unwrap().write_all(js.as_bytes()).unwrap();let out=child.wait_with_output().unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
}

#[test]
fn owner_retries_uncertain_ack_before_allowing_another_poll() {
 let receipt=PollResultReceipt{generation:"ab".repeat(16),sequence:7};
 let item=PollResultAck{session:"nt_1".into(),receipt:receipt.clone(),keys:vec!["request-1".into()]};
 let mut pending=Some(item.clone());let mut attempts=0;
 let ready=advance_poll_result_ack(&mut pending,|got|{attempts+=1;assert_eq!(got,&item);false});
 assert!(!ready);assert_eq!(pending,Some(item.clone()));
 let ready=advance_poll_result_ack(&mut pending,|got|{attempts+=1;assert_eq!(got,&item);true});
 assert!(ready);assert_eq!(pending,None);assert_eq!(attempts,2);
 assert!(advance_poll_result_ack(&mut pending,|_|panic!("no pending receipt")));
}

#[test]
fn entropy_failure_keeps_results_and_oversize_unknown_is_acknowledged() {
 let poll=build_bound_poll_js("nt_1").unwrap();
 let receipt=PollResultReceipt{generation:"33".repeat(16),sequence:1};
 let ack=build_poll_result_ack_js("nt_1",&receipt,&["big".into()]).unwrap();
 let js=r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__,ack=__ACK__;
const R={sid:'nt_1',sends:{},sendResults:{big:{status:'done',msgId:'x'.repeat(530000)}},recv:[],recvBytes:0,rejected:0,overflow:0};
let fail=true;const fakeProcess={getBuiltinModule:()=>({randomBytes:()=>{if(fail)throw Error('entropy unavailable');return Buffer.from('33'.repeat(16),'hex')}})};
const c={__caligo_res:R,process:fakeProcess,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
assert.match(vm.runInContext(poll,c),/^ERR:/);assert.equal(R.sendResults.big.msgId.length,530000);
fail=false;const batch=JSON.parse(vm.runInContext(poll,c));assert.equal(batch.sendResults.big.status,'unknown');
assert.equal(batch.sendResults.big.reason,'result_exceeds_poll_budget');assert.ok(R.sendResults.big);
assert.equal(vm.runInContext(ack,c),'ACKED');assert.equal(R.sendResults.big,undefined);
"#.replace("__POLL__",&serde_json::to_string(&poll).unwrap()).replace("__ACK__",&serde_json::to_string(&ack).unwrap());
 let mut child=std::process::Command::new("node").stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
 child.stdin.take().unwrap().write_all(js.as_bytes()).unwrap();let out=child.wait_with_output().unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
}

#[test]
fn leased_result_crosses_decoder_and_resident_before_acknowledgement() {
 use caligo_bridge::host_adapter::{HostAdapter,HostError,HostOp,HostOpResult};
 use caligo_bridge::resident::{Resident,ResidentLimits,ResidentNotice,SendOutcome,OwnedRequest,SubmitVerdict};
 use caligo_model::wrapper_message::WrapperBinding;
 use std::io::{BufRead,BufReader};
 struct Host;
 impl HostAdapter for Host {
  fn owner_thread_id(&self)->u64{1} fn current_thread_id(&self)->u64{1}
  fn env_valid(&self)->bool{true} fn current_context_ok(&self)->bool{true}
  fn native_op(&mut self,op:HostOp)->Result<HostOpResult,HostError>{match op {
   HostOp::ListenerAdd=>Ok(HostOpResult::ListenerDeferred),
   HostOp::SendText{..}=>Ok(HostOpResult::SendFired{mid:"request-1".into()}),
   _=>panic!("unexpected host operation"),
  }}
 }
 let node=r#"
const vm=require('node:vm'),readline=require('node:readline');
const R={sid:'nt_1',sends:{},sendResults:{'request-1':{status:'done',msgId:'native-1'},orphan:{status:'unknown',reason:'timeout'}},recv:[],recvBytes:0,rejected:0,overflow:0};
const c={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
readline.createInterface({input:process.stdin}).on('line',line=>{const q=JSON.parse(line);
 if(q.op==='quit'){process.exit(0);return;} const result=q.op==='run'?vm.runInContext(q.script,c):Object.keys(R.sendResults);
 process.stdout.write(JSON.stringify(result)+'\n');});
"#;
 let mut child=std::process::Command::new("node").args(["-e",node]).stdin(std::process::Stdio::piped())
  .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
 let mut input=child.stdin.take().unwrap();let mut output=BufReader::new(child.stdout.take().unwrap());
 fn rpc(input:&mut std::process::ChildStdin,output:&mut BufReader<std::process::ChildStdout>,q:serde_json::Value)->serde_json::Value {
  writeln!(input,"{}",q).unwrap();input.flush().unwrap();let mut line=String::new();assert!(output.read_line(&mut line).unwrap()>0);serde_json::from_str(&line).unwrap()
 }
 let run=|s:&str|serde_json::json!({"op":"run","script":s});
 let poll=build_bound_poll_js("nt_1").unwrap();
 let lost=rpc(&mut input,&mut output,run(&poll));assert!(lost.as_str().unwrap().contains("native-1"));
 let returned=rpc(&mut input,&mut output,run(&poll));let json=returned.as_str().unwrap();
 let receipt=poll_result_receipt(json).unwrap().unwrap();
 let resident=Resident::new(Host,ResidentLimits::default());resident.bootstrap().unwrap();
 assert_eq!(resident.submit(OwnedRequest::SendText{request_id:"request-1".into(),wire_seq:1,timeout_ms:5000,chat_type:2,peer_uid:"123".into(),text:"sample".into()}),SubmitVerdict::Accepted);
 resident.drain().unwrap();resident.take_results();
 let binding=WrapperBinding{account:"10001".into(),session_generation:1,host_nonce:1,wrapper_session:"nt_1".into()};
 let batch=resident.ingest_wrapper_batch(json,&binding).unwrap();
 let id=batch.send_results["request-1"]["msgId"].as_str().unwrap().to_string();
 let accepted=resident.complete_sends(&[("request-1".into(),id)],&[],&[("orphan".into(),"timeout".into())]);
 assert_eq!(accepted,vec!["request-1".to_string()]);
 assert_eq!(resident.take_results(),vec![ResidentNotice::Outcome(SendOutcome::Success{request_id:"request-1".into(),native_id:"native-1".into()})]);
 let mut pending=Some(PollResultAck{session:"nt_1".into(),receipt,keys:accepted});
 assert!(!advance_poll_result_ack(&mut pending,|_|false));
 assert_eq!(rpc(&mut input,&mut output,serde_json::json!({"op":"inspect"})),serde_json::json!(["request-1","orphan"]));
 assert!(advance_poll_result_ack(&mut pending,|item|{
  let script=build_poll_result_ack_js(&item.session,&item.receipt,&item.keys).unwrap();rpc(&mut input,&mut output,run(&script))==serde_json::json!("ACKED")
 }));
 assert_eq!(rpc(&mut input,&mut output,serde_json::json!({"op":"inspect"})),serde_json::json!(["orphan"]));
 writeln!(input,"{{\"op\":\"quit\"}}").unwrap();drop(input);assert!(child.wait().unwrap().success());
}

#[test]
fn superseded_snapshot_rejects_old_ack_and_keeps_result() {
 let poll=build_bound_poll_js("nt_1").unwrap();
 let token=|n| PollResultReceipt{generation:"44".repeat(16),sequence:n};
 let ack1=build_poll_result_ack_js("nt_1",&token(1),&["r".into()]).unwrap();let ack2=build_poll_result_ack_js("nt_1",&token(2),&["r".into()]).unwrap();
 let js=r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__,ack1=__ACK1__,ack2=__ACK2__;
const R={sid:'nt_1',sends:{},sendResults:{r:{status:'done',msgId:'123'}},recv:[],recvBytes:0,rejected:0,overflow:0};
const fakeProcess={getBuiltinModule:()=>({randomBytes:()=>Buffer.from('44'.repeat(16),'hex')})};
const c={__caligo_res:R,process:fakeProcess,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
const run=s=>vm.runInContext(s,c);JSON.parse(run(poll));JSON.parse(run(poll));
assert.equal(run(ack1),'ERR:receipt_stale');assert.equal(R.sendResults.r.msgId,'123');
assert.equal(run(ack2),'ACKED');assert.equal(R.sendResults.r,undefined);
"#.replace("__POLL__",&serde_json::to_string(&poll).unwrap()).replace("__ACK1__",&serde_json::to_string(&ack1).unwrap()).replace("__ACK2__",&serde_json::to_string(&ack2).unwrap());
 let mut child=std::process::Command::new("node").stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
 child.stdin.take().unwrap().write_all(js.as_bytes()).unwrap();let out=child.wait_with_output().unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
}
