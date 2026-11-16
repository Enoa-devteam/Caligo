use caligo_bridge::qq_v8::{build_image_send_js, build_voice_send_js, D8_POLL_JS, D8_START_JS};
use std::io::Write;

fn check_ambiguous_media_result(image: bool) {
    let hex = if image {
        "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c63105432fe0f000214016604e0de7f0000000049454e44ae426082"
    } else {
        "02232153494c4b5f56330300445566"
    };
    let bytes: Vec<u8> = (0..hex.len()).step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
    let scripts: Vec<String> = (0..6).map(|i| {
        let id = format!("media-{i}");
        if image { build_image_send_js(1,"10001","20002",&id,i+1,5000,&bytes).unwrap() }
        else { build_voice_send_js(1,"10001","20002",&id,i+1,5000,&bytes).unwrap() }
    }).collect();
    let fixture = r#"
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs'),
 path=require('node:path'),os=require('node:os'),crypto=require('node:crypto');
const start=__START__,poll=__POLL__,scripts=__SCRIPTS__,image=__IMAGE__;
const bytes=Buffer.from('__HEX__','hex'),md5=crypto.createHash('md5').update(bytes).digest('hex');
const root=fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()),'caligo-media-result-'));
let listener,resolve,reject,cookie=0,sends=0,reads=0;
const row=(status,id)=>({msgId:'12345',msgSeq:'1',msgRandom:'2',msgTime:'1794567890',
 guildId:'cookie-'+id,chatType:1,msgType:2,subMsgType:0,peerUid:'u_peer',peerUin:'20002',
 senderUid:'u_self',senderUin:'10001',sendStatus:status,
 elements:[image?{elementType:2,elementId:'67890',picElement:{fileSize:String(bytes.length),
 md5HexStr:md5,picWidth:1,picHeight:1,picType:1000,picSubType:0}}
 :{elementType:4,elementId:'67890',pttElement:{fileSize:String(bytes.length),md5HexStr:md5}}]});
const service={addKernelMsgListener(l){listener=l;return 7;},removeKernelMsgListener:()=>true,
 generateMsgUniqueId:()=> 'cookie-'+cookie,
 getRichMediaFilePathForGuild:()=>path.join(root,md5+(image?'.png':'.silk')),
 sendMsg(){sends++;return new Promise((r,j)=>{resolve=r;reject=j;});},
 getMsgsByMsgId(){reads++;return Promise.resolve({result:0,msgList:[row(2,cookie)]});}};
const session={getSessionId:()=>1,getMsgService:()=>service,getMSFService:()=>({getServerTime:()=> 'time'}),
 getProfileService:()=>({getUidByUin:()=>new Map([['20002','u_peer']])})};
const proc={platform:process.platform,getBuiltinModule:process.getBuiltinModule.bind(process),
 _linkedBinding:()=>({NodeIQQNTWrapperSession:{getNTWrapperSession:()=>session}})};
const c={caligoVoiceAccount:'10001',process:proc,Buffer,Promise,Date,Map,JSON,String,Object,Array,Number};
vm.createContext(c);const run=x=>vm.runInContext(x,c),tick=()=>new Promise(r=>setImmediate(r));
const startSend=i=>{cookie=i;assert.equal(JSON.parse(run(scripts[i])).fired,true);};
const take=i=>JSON.parse(run(poll)).sendResults['media-'+i];
const notify=i=>{listener.onAddSendMsg(row(1,i));listener.onMsgInfoListUpdate([row(2,i)]);};
(async()=>{
 assert.match(run(start),/^ARMED/);
 startSend(0);resolve({result:120});await tick();await tick();
 assert.deepEqual(take(0),{status:'unknown',reason:'send_result_ambiguous=120'},'nonzero Promise before callback must remain unknown');
 notify(0);await tick();assert.notEqual(take(0)?.status,'done');
 startSend(1);notify(1);await tick();await tick();resolve({result:120});await tick();await tick();
 assert.deepEqual(take(1),{status:'unknown',reason:'send_result_ambiguous=120'},'callback before nonzero Promise must also stay unknown');
 startSend(2);resolve({result:9007199254740992});await tick();await tick();
 assert.deepEqual(take(2),{status:'unknown',reason:image?'image_submit_unknown':'voice_submit_unknown'});
 startSend(3);reject('private diagnostic must not escape');await tick();await tick();
 assert.deepEqual(take(3),{status:'unknown',reason:image?'image_submit_unknown':'voice_submit_unknown'});
 startSend(4);resolve({result:1.5});await tick();await tick();
 assert.deepEqual(take(4),{status:'unknown',reason:image?'image_submit_unknown':'voice_submit_unknown'});
 startSend(5);resolve({result:0});notify(5);await tick();await tick();
 const final=take(5);assert.equal(final.status,'done');assert.equal(final.msgId,'12345');
 assert.equal(sends,6);assert.equal(reads,2);
})().catch(e=>{console.error(e);process.exitCode=1;}).finally(()=>fs.rmSync(root,{recursive:true,force:true}));
"#.replace("__START__", &serde_json::to_string(D8_START_JS).unwrap())
 .replace("__POLL__", &serde_json::to_string(D8_POLL_JS).unwrap())
 .replace("__SCRIPTS__", &serde_json::to_string(&scripts).unwrap())
 .replace("__IMAGE__", if image {"true"} else {"false"}).replace("__HEX__",hex);
    let mut child = std::process::Command::new("node").arg("--unhandled-rejections=strict")
        .stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(fixture.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn image_nonzero_promise_result_is_unknown_in_both_orders() { check_ambiguous_media_result(true); }

#[test]
fn voice_nonzero_promise_result_is_unknown_in_both_orders() { check_ambiguous_media_result(false); }
