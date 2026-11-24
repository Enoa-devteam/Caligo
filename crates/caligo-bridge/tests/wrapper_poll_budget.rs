use caligo_bridge::qq_v8::D8_POLL_JS;
use std::io::Write;

fn run_poll_fixture(fixture: &str) {
    let script = fixture.replace("__POLL__", &serde_json::to_string(D8_POLL_JS).unwrap());
    let mut child = std::process::Command::new("node").arg("--unhandled-rejections=strict")
        .stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn poll_preserves_terminal_results_that_did_not_fit() {
    run_poll_fixture(r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__;
const sends={},sendResults={};
for(let i=0;i<64;i++)sendResults['r'+i]={status:'done',msgId:'X'.repeat(3072)+i};
sendResults.waiting={status:'pending'};
const recv=Array.from({length:6},()=>({payload:'R'.repeat(60000)}));
const r={sid:'nt_1',sends,sendResults,recv,recvBytes:360000,rejected:0,overflow:0};
const c={__caligo_res:r,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
const run=()=>{const raw=vm.runInContext(poll,c);assert.ok(Buffer.byteLength(raw,'utf8')<=524288);return JSON.parse(raw);};
const first=run();assert.equal(first.items.length,6);
const sent1=Object.keys(first.sendResults).filter(k=>k!=='waiting');assert.ok(sent1.length>0&&sent1.length<64);
for(let i=0;i<64;i++){
 const key='r'+i;assert.equal(Object.hasOwn(r.sendResults,key),!Object.hasOwn(first.sendResults,key),'only emitted terminal results may be cleared');
}
const second=run(),sent2=Object.keys(second.sendResults).filter(k=>k!=='waiting');
assert.equal(new Set([...sent1,...sent2]).size,64);assert.equal(sent1.length+sent2.length,64);
assert.equal(r.sendResults.waiting.status,'pending');
"#);
}

#[test]
fn oversized_terminal_result_becomes_unknown_without_starving_smaller_results() {
    run_poll_fixture(r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__;
const r={sid:'nt_1',sends:{},sendResults:{huge:{status:'done',msgId:'X'.repeat(530000)},
 huge_pending:{status:'pending',data:'Y'.repeat(530000)},
 small:{status:'unknown',reason:'timeout'},waiting:{status:'pending'}},recv:[],recvBytes:0,rejected:0,overflow:0};
const c={__caligo_res:r,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
const raw=vm.runInContext(poll,c);assert.ok(Buffer.byteLength(raw,'utf8')<=524288);const batch=JSON.parse(raw);
assert.deepEqual(batch.sendResults.small,{status:'unknown',reason:'timeout'});
assert.deepEqual(batch.sendResults.huge,{status:'unknown',reason:'result_exceeds_poll_budget'});
assert.equal(Object.hasOwn(r.sendResults,'huge'),false);assert.equal(Object.hasOwn(r.sendResults,'small'),false);
assert.equal(Object.hasOwn(batch.sendResults,'huge_pending'),false);
assert.equal(r.sendResults.huge_pending.status,'pending');
assert.equal(r.sendResults.waiting.status,'pending');
"#);
}

#[test]
fn oversized_terminal_is_deferred_if_unknown_does_not_fit() {
    run_poll_fixture(r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),poll=__POLL__;
const recv=[{payload:'R'.repeat(390000)}],sendResults={fill:{status:'done',msgId:''}};
const base={schema:1,sid:'nt_1',items:recv,rejected:0,overflow:0,sendResults,
 imageShapes:[],mediaDiagnostics:[],textSendTrace:[],textSendTraceGeneration:0,textSendTraceOverflow:false};
const remaining=524288-Buffer.byteLength(JSON.stringify(base),'utf8')-24;
assert.ok(remaining>0);sendResults.fill.msgId='X'.repeat(remaining);
sendResults.huge={status:'done',msgId:'H'.repeat(530000)};
const r={sid:'nt_1',sends:{},sendResults,recv,recvBytes:390000,rejected:0,overflow:0};
const c={__caligo_res:r,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
const run=()=>{const raw=vm.runInContext(poll,c);assert.ok(Buffer.byteLength(raw,'utf8')<=524288);return JSON.parse(raw);};
const first=run();assert.equal(first.sendResults.fill.status,'done');assert.equal(Object.hasOwn(first.sendResults,'huge'),false);
assert.equal(r.sendResults.huge.status,'done');
const second=run();assert.deepEqual(second.sendResults.huge,{status:'unknown',reason:'result_exceeds_poll_budget'});
assert.equal(Object.hasOwn(r.sendResults,'huge'),false);
"#);
}
