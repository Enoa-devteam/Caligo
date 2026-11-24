use caligo_bridge::qq_v8::build_bound_poll_js;
use caligo_model::wrapper_message::{decode_d8_batch,WrapperBinding};
use std::io::Write;
#[test]
fn rejected_batch_shapes_and_overbudget_base_preserve_state_before_retry() {
 let poll=build_bound_poll_js("nt_1").unwrap();
 let js=r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__,outputs=[];
for(const mode of ['invalid_rejected','invalid_overflow','invalid_items','oversized_base']) {
 const R={sid:'nt_1',sends:{},sendResults:{r1:{status:'done',msgId:'123'}},recv:[],recvBytes:5,rejected:0,overflow:0,
  imageShapes:[],mediaDiagnostics:[],textSendTrace:[]};
 if(mode==='invalid_rejected')R.rejected=NaN;
 if(mode==='invalid_overflow')R.overflow=-1;
 if(mode==='invalid_items')R.recv={not:'an array'};
 if(mode==='oversized_base')R.textSendTrace=['x'.repeat(530000)];
 const c={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
 const before=R.recv,trace=R.textSendTrace,bytes=R.recvBytes;
 assert.equal(vm.runInContext(poll,c),'ERR:poll_batch_invalid',mode);
 assert.equal(R.recv,before);assert.equal(R.textSendTrace,trace);assert.equal(R.recvBytes,bytes);assert.equal(R.sendResults.r1.msgId,'123');
 R.rejected=0;R.overflow=0;R.recv=[];R.textSendTrace=[];
 outputs.push(JSON.parse(vm.runInContext(poll,c)));
}
process.stdout.write(JSON.stringify(outputs));
"#.replace("__POLL__",&serde_json::to_string(&poll).unwrap());
 let mut child=std::process::Command::new("node").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
 child.stdin.take().unwrap().write_all(js.as_bytes()).unwrap();let out=child.wait_with_output().unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
 let batches:Vec<serde_json::Value>=serde_json::from_slice(&out.stdout).unwrap();
 let binding=WrapperBinding{account:"10001".into(),session_generation:1,host_nonce:1,wrapper_session:"nt_1".into()};
 for batch in batches {let v=decode_d8_batch(&batch.to_string(),&binding).unwrap();assert_eq!(v.send_results["r1"]["msgId"],"123");}
}
