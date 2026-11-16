use caligo_bridge::qq_v8::{build_image_send_js, build_send_js, build_voice_send_js};
use std::io::Write;

#[test]
fn send_results_capacity_blocks_text_image_and_voice_before_submit() {
    let png_hex="89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63105432fe0f000214016604e0de7f0000000049454e44ae426082";
    let png:Vec<u8>=(0..png_hex.len()).step_by(2).map(|i|u8::from_str_radix(&png_hex[i..i+2],16).unwrap()).collect();
    let silk_hex="02232153494c4b5f56330300445566";
    let silk:Vec<u8>=(0..silk_hex.len()).step_by(2).map(|i|u8::from_str_radix(&silk_hex[i..i+2],16).unwrap()).collect();
    let scripts=[build_send_js(1,"20002","hello","10001","new-text",1,5000),
        build_image_send_js(1,"10001","20002","new-image",2,5000,&png).unwrap(),
        build_voice_send_js(1,"10001","20002","new-voice",3,5000,&silk).unwrap()];
    let js=r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),scripts=__SCRIPTS__;
function setup(n) {
 let submits=0,storage=0;
 const results={};for(let i=0;i<n;i++)results['old-'+i]={status:'unknown',reason:'timeout'};
 const service={generateMsgUniqueId:()=> 'cookie-1',sendMsg(){submits++;return new Promise(()=>{});},
 getRichMediaFilePathForGuild(){storage++;throw new Error('unexpected storage');}};
 const session={getMSFService:()=>({getServerTime:()=> 'time'}),
 getProfileService:()=>({getUidByUin:()=>new Map([['20002','u_peer']])})};
 const R={voiceAccount:'10001',sends:{},sendResults:results,ms:service,session};
 const context={__caligo_res:R,process,Buffer,Date,JSON,Object,Array,Number,String,Map,Set,Promise};vm.createContext(context);
 return {context,R,get submits(){return submits;},get storage(){return storage;}};
}
for(const script of scripts){
 const e=setup(128);const result=JSON.parse(vm.runInContext(script,e.context));
 assert.deepEqual(result,{err:'send_capacity_or_duplicate'});assert.equal(e.submits,0);assert.equal(e.storage,0);
 assert.equal(Object.keys(e.R.sendResults).length,128);assert.equal(Object.keys(e.R.sends).length,0);
}
const e=setup(127);const result=JSON.parse(vm.runInContext(scripts[0],e.context));
assert.equal(result.fired,true);assert.equal(e.submits,1);assert.equal(Object.keys(e.R.sendResults).length,128);
"#.replace("__SCRIPTS__",&serde_json::to_string(&scripts).unwrap());
    let mut child=std::process::Command::new("node").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(js.as_bytes()).unwrap();let out=child.wait_with_output().unwrap();
    assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
}
