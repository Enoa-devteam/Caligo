use caligo_bridge::qq_v8::{build_bound_poll_js,build_poll_result_ack_js,PollResultReceipt};
use std::io::Write;
fn run(fixture:&str){
 let poll=build_bound_poll_js("nt_1").unwrap();
 let receipt=|n| PollResultReceipt{generation:"66".repeat(16),sequence:n};
 let mut source=fixture.replace("__POLL__",&serde_json::to_string(&poll).unwrap());
 for n in 1..=6 {let ack=build_poll_result_ack_js("nt_1",&receipt(n),&[]).unwrap();source=source.replace(&format!("__ACK{n}__"),&serde_json::to_string(&ack).unwrap());}
 let ack_new=build_poll_result_ack_js("nt_1",&receipt(2),&["fresh".into()]).unwrap();
 let ack_update=build_poll_result_ack_js("nt_1",&receipt(3),&["orphan".into()]).unwrap();
 let ack_huge=build_poll_result_ack_js("nt_1",&receipt(2),&["huge".into()]).unwrap();
 source=source.replace("__ACKNEW__",&serde_json::to_string(&ack_new).unwrap()).replace("__ACKUPDATE__",&serde_json::to_string(&ack_update).unwrap())
  .replace("__ACKHUGE__",&serde_json::to_string(&ack_huge).unwrap());
 let mut child=std::process::Command::new("node").stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();child.stdin.take().unwrap().write_all(source.as_bytes()).unwrap();let out=child.wait_with_output().unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
}
#[test]
fn unmatched_result_retries_with_bounded_monotonic_backoff() {run(r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__,ack=[null,__ACK1__,__ACK2__,__ACK3__,__ACK4__,__ACK5__,__ACK6__];
let clock=0n;const process={getBuiltinModule:()=>({randomBytes:()=>Buffer.from('66'.repeat(16),'hex')}),hrtime:{bigint:()=>clock}};
const R={sid:'nt_1',sends:{},sendResults:{orphan:{status:'unknown',reason:'timeout'}},recv:[],recvBytes:0,rejected:0,overflow:0};
const c={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
const read=()=>JSON.parse(vm.runInContext(poll,c)),confirm=n=>assert.equal(vm.runInContext(ack[n],c),'ACKED');
assert.equal(read().resultReceipt.sequence,1);confirm(1);clock=100000000n;confirm(1);
assert.equal(read().sendResults.orphan,undefined);assert.equal(R.sendResults.orphan.status,'unknown');
clock=249000000n;assert.equal(read().sendResults.orphan,undefined);
clock=250000000n;assert.equal(read().resultReceipt.sequence,2);confirm(2);
clock=749000000n;assert.equal(read().sendResults.orphan,undefined);
clock=750000000n;assert.equal(read().resultReceipt.sequence,3);confirm(3);
clock=1750000000n;assert.equal(read().resultReceipt.sequence,4);confirm(4);
clock=3749000000n;assert.equal(read().sendResults.orphan,undefined);
clock=3750000000n;assert.equal(read().resultReceipt.sequence,5);confirm(5);
clock=5750000000n;assert.equal(read().resultReceipt.sequence,6);confirm(6);assert.ok(R.sendResults.orphan);
"#);}
#[test]
fn new_and_changed_results_bypass_other_objects_backoff() {run(r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__,ack1=__ACK1__,ackNew=__ACKNEW__,ackUpdate=__ACKUPDATE__;
let clock=0n;const process={getBuiltinModule:()=>({randomBytes:()=>Buffer.from('66'.repeat(16),'hex')}),hrtime:{bigint:()=>clock}};
const R={sid:'nt_1',sends:{},sendResults:{orphan:{status:'unknown',reason:'timeout'}},recv:[],recvBytes:0,rejected:0,overflow:0};
const c={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);const read=()=>JSON.parse(vm.runInContext(poll,c));
read();assert.equal(vm.runInContext(ack1,c),'ACKED');R.recv=[{synthetic:'event'}];R.sendResults.fresh={status:'done',msgId:'20'};
const next=read();assert.equal(next.items.length,1);assert.equal(next.sendResults.orphan,undefined);assert.equal(next.sendResults.fresh.msgId,'20');
assert.equal(vm.runInContext(ackNew,c),'ACKED');R.sendResults.orphan={status:'done',msgId:'30'};
assert.equal(read().sendResults.orphan.msgId,'30');assert.equal(vm.runInContext(ackUpdate,c),'ACKED');assert.equal(R.sendResults.orphan,undefined);
"#);}
#[test]
fn unavailable_clock_fails_open_to_immediate_retry() {run(r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__,ack=__ACK1__,ack2=__ACK2__;
let fail=true;const process={getBuiltinModule:()=>({randomBytes:()=>Buffer.from('66'.repeat(16),'hex')}),hrtime:{bigint:()=>{if(fail)throw Error('unavailable');return 0n;}}};
const R={sid:'nt_1',sends:{},sendResults:{orphan:{status:'unknown',reason:'timeout'}},recv:[],recvBytes:0,rejected:0,overflow:0};
const c={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);const read=()=>JSON.parse(vm.runInContext(poll,c));
read();assert.equal(vm.runInContext(ack,c),'ACKED');assert.equal(read().sendResults.orphan.status,'unknown');
fail=false;assert.equal(vm.runInContext(ack2,c),'ACKED');assert.equal(read().sendResults.orphan,undefined);
fail=true;assert.equal(read().sendResults.orphan.status,'unknown');
"#);}

#[test]
fn backward_clock_cannot_hide_an_unacknowledged_result() {run(r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__,ack=__ACK1__;
let clock=1000000000n;const process={getBuiltinModule:()=>({randomBytes:()=>Buffer.from('66'.repeat(16),'hex')}),hrtime:{bigint:()=>clock}};
const R={sid:'nt_1',sends:{},sendResults:{orphan:{status:'unknown'}},recv:[],recvBytes:0,rejected:0,overflow:0};
const c={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);const read=()=>JSON.parse(vm.runInContext(poll,c));
read();assert.equal(vm.runInContext(ack,c),'ACKED');clock=900000000n;assert.equal(read().sendResults.orphan.status,'unknown');
"#);}
#[test]
fn deferral_capacity_is_bounded_and_overflow_is_not_hidden() {run(r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__,ack=__ACK1__;
const process={getBuiltinModule:()=>({randomBytes:()=>Buffer.from('66'.repeat(16),'hex')}),hrtime:{bigint:()=>0n}};
const results={};for(let i=0;i<129;i++)results['r'+i]={status:'unknown'};
const R={sid:'nt_1',sends:{},sendResults:results,recv:[],recvBytes:0,rejected:0,overflow:0};
const c={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);const read=()=>JSON.parse(vm.runInContext(poll,c));
assert.equal(Object.keys(read().sendResults).length,129);assert.equal(vm.runInContext(ack,c),'ACKED');
assert.equal(R.resultDeferrals.size,128);assert.equal(Object.keys(R.sendResults).length,129);assert.equal(Object.keys(read().sendResults).length,1);
"#);}

#[test]
fn budget_fallback_does_not_reintroduce_deferred_result() {run(r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__,ack1=__ACK1__,ackHuge=__ACKHUGE__;
let clock=0n;const process={getBuiltinModule:()=>({randomBytes:()=>Buffer.from('66'.repeat(16),'hex')}),hrtime:{bigint:()=>clock}};
const R={sid:'nt_1',sends:{},sendResults:{orphan:{status:'unknown'}},recv:[],recvBytes:0,rejected:0,overflow:0};
const c={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);const read=()=>JSON.parse(vm.runInContext(poll,c));
read();assert.equal(vm.runInContext(ack1,c),'ACKED');R.sendResults.huge={status:'done',msgId:'x'.repeat(530000)};
const batch=read();assert.equal(batch.sendResults.orphan,undefined);assert.equal(batch.sendResults.huge.reason,'result_exceeds_poll_budget');
assert.equal(vm.runInContext(ackHuge,c),'ACKED');assert.ok(R.sendResults.orphan);assert.equal(R.sendResults.huge,undefined);
clock=250000000n;assert.equal(read().sendResults.orphan.status,'unknown');
"#);}

#[test]
fn malformed_deferral_cannot_hide_a_retained_result() {run(r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__,ack=__ACK1__;
const process={getBuiltinModule:()=>({randomBytes:()=>Buffer.from('66'.repeat(16),'hex')}),hrtime:{bigint:()=>0n}};
const R={sid:'nt_1',sends:{},sendResults:{orphan:{status:'unknown'}},recv:[],recvBytes:0,rejected:0,overflow:0};
const c={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);const read=()=>JSON.parse(vm.runInContext(poll,c));
read();assert.equal(vm.runInContext(ack,c),'ACKED');R.resultDeferrals.set('orphan',{value:R.sendResults.orphan,since:0n,until:'broken',count:4});
assert.equal(read().sendResults.orphan.status,'unknown');assert.equal(R.resultDeferrals.size,0);
"#);}
