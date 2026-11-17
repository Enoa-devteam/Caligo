use caligo_bridge::qq_v8::{build_result_pressure_js,sanitize_result_pressure,read_result_pressure};
use std::io::Write;
#[test]
fn pressure_sample_is_read_only_session_bound_and_does_not_expose_result_data() {
 let sample=build_result_pressure_js("nt_1").unwrap();let wrong=build_result_pressure_js("nt_\";SECRET").unwrap();
 assert!(build_result_pressure_js("").is_none());assert_eq!(read_result_pressure("").unwrap(),None);assert!(build_result_pressure_js(&"x".repeat(129)).is_none());
 let js=r#"
const realProcess=globalThis.process;const assert=require('node:assert/strict'),vm=require('node:vm'),sample=__SAMPLE__,wrong=__WRONG__,results={};
results['SECRET-REQUEST']={status:'unknown',reason:'SECRET-REASON'};results.other={status:'done',msgId:'SECRET-MESSAGE'};results.pending={status:'pending'};
let now=100n,clock=true;const process={hrtime:{bigint:()=>{if(!clock)throw Error('clock');return now;}}};
const R={sid:'nt_1',sendResults:results,resultDeferrals:new Map()};
const active={value:results['SECRET-REQUEST'],since:50n,until:150n,count:1};
const expired={value:results.other,since:0n,until:90n,count:1};R.resultDeferrals.set('SECRET-REQUEST',active);R.resultDeferrals.set('other',expired);
const c={__caligo_res:R,process,Map,Buffer,Date,JSON,Object,Array,Number,String};vm.createContext(c);
const read=()=>vm.runInContext(sample,c),out=[];assert.equal(vm.runInContext(wrong,c),'ERR:pressure_session');
const first=read();assert.ok(!first.includes('SECRET'));out.push(JSON.parse(first));
assert.equal(R.sendResults,results);assert.equal(R.resultDeferrals.size,2);assert.equal(R.resultDeferrals.get('SECRET-REQUEST'),active);
clock=false;out.push(JSON.parse(read()));clock=true;
R.resultDeferrals.set('other',{value:results.other,since:0n,until:'bad',count:1});out.push(JSON.parse(read()));
R.resultDeferrals=undefined;out.push(JSON.parse(read()));
for(let i=0;i<125;i++)results['more-'+i]={status:'pending'};out.push(JSON.parse(read()));
results.extra={status:'pending'};out.push(JSON.parse(read()));
R.sendResults=null;out.push(read());R.sendResults=new Map();out.push(read());realProcess.stdout.write(JSON.stringify(out));
"#.replace("__SAMPLE__",&serde_json::to_string(&sample).unwrap()).replace("__WRONG__",&serde_json::to_string(&wrong).unwrap());
 let mut child=std::process::Command::new("node").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();child.stdin.take().unwrap().write_all(js.as_bytes()).unwrap();let out=child.wait_with_output().unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
 let samples:Vec<serde_json::Value>=serde_json::from_slice(&out.stdout).unwrap();
 let expected=[(3,false,Some(1)),(3,false,None),(3,false,None),(3,false,Some(0)),(128,false,Some(0)),(128,true,None)];
 for (sample,(entries,over,delayed)) in samples.iter().take(6).zip(expected) {
  let raw=sample.to_string();let detail=sanitize_result_pressure(&raw).unwrap();assert!(!detail.contains("SECRET"));
  let v:serde_json::Value=serde_json::from_str(&detail).unwrap();assert_eq!(v["entries"],entries);assert_eq!(v["over_capacity"],over);
  assert_eq!(v["saturated"],entries==128);assert_eq!(v["capacity"],128);assert_eq!(v["delayed"].as_u64(),delayed);
 }
 assert_eq!(samples[6],"ERR:pressure_state");assert!(sanitize_result_pressure(samples[6].as_str().unwrap()).is_none());
 assert_eq!(samples[7],"ERR:pressure_state");assert!(sanitize_result_pressure(samples[7].as_str().unwrap()).is_none());
}
#[test]
fn sanitizer_rejects_invalid_or_impossible_counts_and_strips_extra_fields() {
 for raw in ["NOT_ARMED",r#"{}"#,r#"{"entries":129,"over_capacity":false,"delayed":0}"#,
  r#"{"entries":-1,"over_capacity":false,"delayed":0}"#,r#"{"entries":1.5,"over_capacity":false,"delayed":0}"#,
  r#"{"entries":1,"over_capacity":true,"delayed":null}"#,r#"{"entries":128,"over_capacity":true,"delayed":1}"#,
  r#"{"entries":1,"over_capacity":false,"delayed":2}"#,r#"{"entries":1,"over_capacity":"yes","delayed":0}"#] {
   assert!(sanitize_result_pressure(raw).is_none(),"{raw}");
 }
 assert!(sanitize_result_pressure(&" ".repeat(1025)).is_none());
 let raw=r#"{"entries":128,"over_capacity":false,"delayed":2,"request":"SECRET","nested":{"reason":"SECRET"}}"#;
 let detail=sanitize_result_pressure(raw).unwrap();assert!(!detail.contains("SECRET"));
 let value:serde_json::Value=serde_json::from_str(&detail).unwrap();assert_eq!(value["saturated"],true);assert_eq!(value["delayed"],2);
}
