//! Fixed-profile PNG query contracts against a synthetic Node host; never loads QQ.
#[path = "../src/qq_query.rs"]
mod qq_query;

use caligo_model::capability::{HostQuery, QueryData, QueryOutcome, QueryScope};

fn scope() -> QueryScope {
    QueryScope {
        account: "10001".into(),
        host_nonce: 7,
        session_generation: 8,
        connection_epoch: 9,
    }
}

fn query(download: bool, chat_type: u32, peer: &str, sender: &str) -> HostQuery {
    if download {
        HostQuery::ImageDownload {
            chat_type,
            peer_uid: peer.into(),
            sender_uin: sender.into(),
            message_id: "90071992547409939999".into(),
            element_id: "9007199254740994".into(),
        }
    } else {
        HostQuery::ImageInfo {
            chat_type,
            peer_uid: peer.into(),
            sender_uin: sender.into(),
            message_id: "90071992547409939999".into(),
            element_id: "9007199254740994".into(),
        }
    }
}

fn run_node(script: &str) -> String {
    use std::io::Write;
    let mut child = std::process::Command::new("node")
        .args(["--unhandled-rejections=strict", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

const FIXTURE: &str = r#"
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQVDL+DwACFAFmBODefwAAAABJRU5ErkJggg==','base64');
const md5 = crypto.createHash('md5').update(png).digest('hex');
const msgId='90071992547409939999', elementId='9007199254740994';
let chatType=1, peerUid='u_image_peer', peerUin='20002', senderUin='20002';
function fresh() {
  return {msgId,chatType,peerUid,peerUin,senderUid:'u_image_peer',senderUin,sendStatus:2,
    elements:[{elementType:2,elementId,picElement:{fileSize:png.length,picWidth:1,picHeight:1,
      picType:1001,picSubType:0,md5HexStr:md5.toUpperCase(),filePath:'private-path',
      sourcePath:'private-source',fileUuid:'private-token',originImageUrl:'private-url'}}]};
}
const tick = () => new Promise(resolve => setImmediate(resolve));
const call = source => JSON.parse(eval(source));
"#;

#[test]
fn image_metadata_requires_exact_ordinary_png_and_excludes_private_fields() {
    let q = query(false, 1, "u_image_peer", "20002");
    assert!(q.validate().is_ok());
    assert!(!qq_query::is_supported(&q));
    let quote = |s: String| serde_json::to_string(&s).unwrap();
    let script = format!(
        "const start={},poll={};\n",
        quote(qq_query::build_query_start_js("image", &scope(), &q, 1000)),
        quote(qq_query::build_query_poll_js("image", &scope()))
    ) + FIXTURE
        + r#"
let message;
const service = {getMsgsByMsgId(peer,ids) {
  assert.equal(this,service);
  assert.deepEqual(peer,{chatType:1,peerUid:'u_image_peer',guildId:''});
  assert.deepEqual(ids,[msgId]); return Promise.resolve({result:0,msgList:[message]});
}};
async function read(m) {
  message=m; globalThis.__caligo_d8={sid:'nt_1',session:{getMsgService:()=>service}};
  assert.equal(call(start).status,'pending'); await tick(); return call(poll);
}
(async () => {
  for (const mutate of [
    m=>m.msgId='999', m=>m.chatType=2, m=>m.peerUid='u_other', m=>m.senderUin='20003',
    m=>m.senderUid='u_wrong', m=>m.elements[0].elementId=9007199254740994,
    m=>m.elements.push(m.elements[0]), m=>m.elements[0].elementType=4,
    m=>m.elements[0].picElement.picType=1000, m=>delete m.elements[0].picElement.picSubType,
    m=>m.elements[0].picElement.picSubType=1, m=>m.elements[0].picElement.fileSize='1e3',
    m=>m.elements[0].picElement.fileSize=16777217, m=>m.elements[0].picElement.fileSize=1.5,
    m=>m.elements[0].picElement.md5HexStr='bad', m=>m.elements[0].picElement.picWidth=0,
    m=>m.elements[0].picElement.picHeight=32769,
    m=>{m.elements[0].picElement.picWidth=32768;m.elements[0].picElement.picHeight=32768;},
  ]) {
    const m=fresh(); mutate(m); assert.equal((await read(m)).status,'invalid_response');
  }
  const stringSize=fresh(); stringSize.elements[0].picElement.fileSize=String(png.length);
  assert.equal((await read(stringSize)).status,'success');
  process.stdout.write(JSON.stringify(await read(fresh())));
})().catch(e=>{console.error(e);process.exitCode=1;});
"#;
    let json = run_node(&script);
    assert!(!json.contains("private"));
    let QueryOutcome::Success(QueryData::ImageInfo(info)) =
        qq_query::decode_query_result(&q, &scope(), &json).unwrap()
    else {
        panic!("image metadata missing")
    };
    assert_eq!((info.width, info.height, info.pic_type), (1, 1, 1001));
    for field in [
        "chat_type",
        "sender_uin",
        "message_id",
        "element_id",
        "pic_type",
        "width",
        "size",
        "md5",
    ] {
        let mut value = serde_json::to_value(&info).unwrap();
        value[field] = match field {
            "chat_type" => 2.into(),
            "pic_type" => 1000.into(),
            "width" | "size" => 0.into(),
            _ => "bad".into(),
        };
        let wrong = serde_json::from_value(value).unwrap();
        assert!(
            QueryData::ImageInfo(wrong)
                .validate_for(&q, &scope())
                .is_err(),
            "{field}"
        );
    }
}

#[test]
fn image_download_binds_callback_file_scope_integrity_and_lifetime() {
    use caligo_bridge::qq_v8::{D8_START_JS, D8_STOP_JS};
    let q = query(true, 1, "u_image_peer", "20002");
    let quote = |s: &str| serde_json::to_string(s).unwrap();
    let script = format!(
        "const arm={},stop={},start={},poll={},cancel={},group={},sent={},sentPilot={},sentGroupPilot={},sentGroup={},incomingGroupPilot={},drain={};\n",
        quote(D8_START_JS),
        quote(D8_STOP_JS),
        quote(&qq_query::build_query_start_js("image", &scope(), &q, 1000)),
        quote(&qq_query::build_query_poll_js("image", &scope())),
        quote(&qq_query::build_query_cancel_js("image", &scope())),
        quote(&qq_query::build_query_start_js(
            "image",
            &scope(),
            &query(true, 2, "30003", "20002"),
            1000
        )),
        quote(&qq_query::build_query_start_js(
            "image",
            &scope(),
            &query(true, 1, "u_image_peer", "10001"),
            1000
        )),
        quote(&qq_query::build_profile_query_start_js(
            "image",
            &scope(),
            &query(true, 1, "u_image_peer", "10001"),
            1000,
            true
        )),
        quote(&qq_query::build_profile_query_start_js(
            "image",
            &scope(),
            &query(true, 2, "30003", "10001"),
            1000,
            true
        )),
        quote(&qq_query::build_query_start_js(
            "image",
            &scope(),
            &query(true, 2, "30003", "10001"),
            1000
        )),
        quote(&qq_query::build_profile_query_start_js(
            "image",
            &scope(),
            &query(true, 2, "30003", "20002"),
            1000,
            true
        )),
        quote(caligo_bridge::qq_v8::D8_POLL_JS),
    ) + FIXTURE
        + r#"
const fs=require('node:fs'),path=require('node:path');
const cacheRoot=fs.mkdtempSync(path.join(require('node:os').tmpdir(),'caligo-cache-lab-'));
const cachePath=path.join(cacheRoot,md5+'.png');let cacheBytes=png,invalidCache=false;
let listener,requested,calls=0,mode='pending',delayed,mutate=m=>{};
const service={sendMsg(){},addKernelMsgListener(v){listener=v;return 7;},removeKernelMsgListener(){},
  getRichMediaFilePathForGuild(args){
    assert.deepEqual(args,{md5HexStr:md5,fileName:md5+'.png',elementType:2,elementSubType:0,
      thumbSize:0,needCreate:true,downloadType:1,file_uuid:''});
    return invalidCache?'relative.png':cachePath;
  },
  getMsgsByMsgId(peer,ids) {
    assert.equal(this,service);assert.deepEqual(peer,{chatType,peerUid,guildId:''});assert.deepEqual(ids,[msgId]);
    const m=fresh();mutate(m);const out={result:0,msgList:[m]};
    if(mode==='delayed')return new Promise(resolve=>delayed=()=>resolve(out));
    return Promise.resolve(out);
  },downloadRichMedia(args) {
    assert.equal(this,service);calls++;requested=args.filePath;
    assert.ok(path.isAbsolute(requested));
    assert.deepEqual(args,{msgId,elementId,peerUid,chatType,fileModelId:'0',downSourceType:0,
      downloadSourceType:0,triggerType:1,thumbSize:0,downloadType:1,filePath:requested});
    if(mode==='sync'){fs.writeFileSync(requested,png);listener.onRichMediaDownloadComplete(complete());}
    if(mode==='cache'){fs.writeFileSync(cachePath,cacheBytes);listener.onRichMediaDownloadComplete(complete({filePath:cachePath}));}
    if(mode==='reject')return Promise.reject(new Error('outcome unknown'));
    return Promise.resolve({result:0});
  }};
function complete(extra={}) {return {msgId,msgElementId:elementId,chatType,fileErrCode:'0',filePath:requested,...extra};}
const session={getSessionId:()=>1,getMsgService:()=>service};
process._linkedBinding=()=>({NodeIQQNTWrapperSession:{getNTWrapperSession:()=>session}});
assert.ok(eval(arm).startsWith('ARMED'));
async function begin(source=start){assert.equal(call(source).status,'pending');await tick();}
function finish(data=png,extra={}){const target=requested;fs.writeFileSync(target,data);listener.onRichMediaDownloadComplete(complete(extra));return target;}
(async()=>{
  mode='delayed';assert.equal(call(start).status,'pending');assert.equal(call(cancel).status,'cancelled');delayed();await tick();assert.equal(calls,0);
  mode='pending';await begin();
  for(const wrong of [{msgId:'99'},{msgElementId:'99'},{chatType:2},{filePath:requested+'.wrong'}]) {
    listener.onRichMediaDownloadComplete(complete(wrong));assert.equal(call(poll).status,'pending');
  }
  const diagnostic=call(drain).mediaDiagnostics;
  assert.deepEqual(diagnostic.slice(0,2),[
    {media:'image',stage:'dispatch'}, {media:'image',stage:'promise',result:'accepted'}
  ]);
  const pathMismatch=diagnostic.find(row=>row.stage==='callback' && row.path_matches===false);
  assert.equal(pathMismatch.message_matches,true);assert.equal(pathMismatch.element_matches,true);
  assert.equal(pathMismatch.pending,true);assert.equal(pathMismatch.slot_matches,true);
  assert.ok(!JSON.stringify(diagnostic).includes(requested));
  assert.deepEqual(call(drain).mediaDiagnostics,[]);
  for(const [value,label] of [[2,'group'],[0,'zero'],['1','string_private'],['2','string_group'],
      [undefined,'missing'],[null,'null'],['SECRET','other'],[{},'other']]) {
    listener.onRichMediaDownloadComplete(complete({chatType:value}));
    assert.equal(call(poll).status,'pending','diagnostics must preserve strict chat matching');
    const entries=call(drain).mediaDiagnostics;
    assert.equal(entries.length,1);assert.equal(entries[0].callback_chat,label);
    assert.equal(entries[0].chat_matches,false);assert.ok(!JSON.stringify(entries).includes('SECRET'));
  }
  listener.onRichMediaDownloadComplete(complete({msgId:'wrong'}));
  assert.equal(call(drain).mediaDiagnostics[0].callback_chat,'private');
  globalThis.__caligo_d8.queryPending.get('image').deadline=Date.now()-1;
  assert.equal(call(poll).status,'timeout');assert.equal(call(start).code,409);
  const old=finish();assert.equal(fs.existsSync(path.dirname(old)),false);
  await begin();assert.notEqual(requested,old);
  listener.onRichMediaDownloadComplete(complete({filePath:old}));assert.equal(call(poll).status,'pending');
  finish(Buffer.alloc(png.length));assert.equal(call(poll).status,'invalid_response');
  await begin();finish(Buffer.concat([png,Buffer.from([0])]));assert.equal(call(poll).status,'invalid_response');
  mutate=m=>m.elements[0].picElement.picWidth=2;
  await begin();finish();assert.equal(call(poll).status,'invalid_response');mutate=m=>{};
  await begin();const rejectPath=requested;listener.onRichMediaDownloadComplete(complete({fileErrCode:'5'}));
  assert.equal(call(poll).status,'rejected');assert.equal(fs.existsSync(path.dirname(rejectPath)),false);
  mode='reject';await begin();assert.equal(call(poll).status,'invalid_response');assert.equal(call(start).code,409);finish();
  mode='sync';await begin();const success=call(poll);assert.equal(success.status,'success');
  assert.equal(success.data.data.png_hex,png.toString('hex'));assert.equal(fs.existsSync(path.dirname(requested)),false);
  chatType=2;peerUid=peerUin='30003';mode='pending';
  mutate=m=>m.peerUin='999';await begin(group);assert.equal(call(poll).status,'invalid_response');mutate=m=>{};
  await begin(group);finish();assert.equal(call(poll).data.data.info.chat_type,2);
  chatType=1;peerUid='u_image_peer';peerUin='20002';senderUin='10001';
  mutate=m=>m.sendStatus=1;await begin(sent);assert.equal(call(poll).status,'invalid_response');mutate=m=>{};
  mode='sync';mutate=m=>m.elements[0].picElement.picType=1000;
  await begin(sent);const own=call(poll);assert.equal(own.data.data.info.sender_uin,'10001');
  assert.equal(own.data.data.info.pic_type,1000);assert.equal(own.data.data.png_hex,png.toString('hex'));mutate=m=>{};
  mode='pending';mutate=m=>m.elements[0].picElement.picType=1000;
  await begin(sentPilot);fs.writeFileSync(cachePath,png);
  for(const wrong of [{msgId:'wrong'},{msgElementId:'wrong'},{chatType:undefined},{chatType:'1'},
      {chatType:0},{chatType:2},{filePath:requested},{filePath:cachePath+'.wrong'}]) {
    listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null,...wrong}));
    assert.equal(call(poll).status,'pending','null chat exception must keep exact message/element/path/type gates');
  }
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(call(poll).status,'success','pinned self-sent private PNG cached callback may have chatType=null');
  assert.equal(fs.existsSync(cachePath),true);assert.equal(fs.existsSync(path.dirname(requested)),false);
  await begin(sent);listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(call(poll).status,'pending','unverified profiles cannot use null chat');
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath}));assert.equal(call(poll).status,'success');
  mutate=m=>{};await begin(sentPilot);
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(call(poll).status,'pending','picType 1001 keeps strict chat');
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath}));assert.equal(call(poll).status,'success');
  mutate=m=>m.elements[0].picElement.picType=1000;
  await begin(sentPilot);fs.writeFileSync(cachePath,Buffer.alloc(png.length));
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(call(poll).status,'invalid_response','null chat never bypasses file integrity');
  assert.equal(fs.existsSync(cachePath),true);fs.writeFileSync(cachePath,png);
  await begin(sentPilot);const expiredSelf=requested;
  globalThis.__caligo_d8.queryPending.get('image').deadline=Date.now()-1;
  assert.equal(call(poll).status,'timeout');listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(fs.existsSync(path.dirname(expiredSelf)),false);assert.equal(fs.existsSync(cachePath),true);
  await begin(sentPilot);const cancelledSelf=requested;assert.equal(call(cancel).status,'cancelled');
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(globalThis.__caligo_d8.queryPending.has('image'),false);
  assert.equal(fs.existsSync(path.dirname(cancelledSelf)),false);assert.equal(fs.existsSync(cachePath),true);
  chatType=2;peerUid=peerUin='30003';await begin(sentGroupPilot);
  for(const wrong of [{msgId:'wrong'},{msgElementId:'wrong'},{chatType:undefined},{chatType:'2'},
      {chatType:0},{chatType:1},{filePath:requested},{filePath:cachePath+'.wrong'}]) {
    listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null,...wrong}));
    assert.equal(call(poll).status,'pending','group null chat keeps exact message/element/path/type gates');
  }
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  const ownGroup=call(poll);assert.equal(ownGroup.status,'success','pinned self-sent group PNG cached callback may have chatType=null');
  assert.equal(ownGroup.data.data.info.chat_type,2);assert.equal(ownGroup.data.data.info.peer_uid,'30003');
  assert.equal(ownGroup.data.data.png_hex,png.toString('hex'));assert.equal(fs.existsSync(cachePath),true);
  assert.equal(fs.existsSync(path.dirname(requested)),false);
  await begin(sentGroup);listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(call(poll).status,'pending','unverified group profile keeps strict chat');
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath}));assert.equal(call(poll).status,'success');
  mutate=m=>m.elements[0].picElement.picType=1001;await begin(sentGroupPilot);
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(call(poll).status,'pending','group picType 1001 keeps strict chat');
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath}));assert.equal(call(poll).status,'success');
  mutate=m=>{m.elements[0].picElement.picType=1000;m.peerUin='wrong';};
  const beforeWrongPeer=calls;await begin(sentGroupPilot);assert.equal(call(poll).status,'invalid_response');
  assert.equal(calls,beforeWrongPeer,'wrong group readback must not dispatch download');
  mutate=m=>m.elements[0].picElement.picType=1000;
  await begin(sentGroupPilot);fs.writeFileSync(cachePath,Buffer.alloc(png.length));
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(call(poll).status,'invalid_response');assert.equal(fs.existsSync(cachePath),true);fs.writeFileSync(cachePath,png);
  await begin(sentGroupPilot);const expiredGroup=requested;
  globalThis.__caligo_d8.queryPending.get('image').deadline=Date.now()-1;
  assert.equal(call(poll).status,'timeout');listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(fs.existsSync(path.dirname(expiredGroup)),false);assert.equal(fs.existsSync(cachePath),true);
  await begin(sentGroupPilot);const cancelledGroup=requested;assert.equal(call(cancel).status,'cancelled');
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(globalThis.__caligo_d8.queryPending.has('image'),false);
  assert.equal(fs.existsSync(path.dirname(cancelledGroup)),false);assert.equal(fs.existsSync(cachePath),true);
  senderUin='20002';mutate=m=>{};await begin(incomingGroupPilot);
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(call(poll).status,'pending','incoming group PNG keeps strict chat');
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath}));assert.equal(call(poll).status,'success');
  chatType=1;peerUid='u_image_peer';peerUin='20002';senderUin='20002';mutate=m=>{};
  await begin();listener.onRichMediaDownloadComplete(complete({filePath:cachePath,chatType:null}));
  assert.equal(call(poll).status,'pending','incoming PNG keeps strict chat');
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath}));assert.equal(call(poll).status,'success');
  senderUin='20002';mode='cache';await begin();
  assert.equal(call(poll).status,'success','registered QQ cache callback must complete the download');
  assert.equal(fs.existsSync(cachePath),true,'QQ cache file must not be deleted');
  assert.equal(fs.existsSync(path.dirname(requested)),false,'owned temporary target is cleaned');
  assert.equal(fs.readFileSync(cachePath).toString('hex'),png.toString('hex'));
  cacheBytes=Buffer.alloc(png.length);await begin();assert.equal(call(poll).status,'invalid_response');cacheBytes=png;
  assert.equal(fs.existsSync(cachePath),true,'failed validation also preserves the QQ cache');
  mode='pending';await begin();const lateTarget=requested;
  globalThis.__caligo_d8.queryPending.get('image').deadline=Date.now()-1;
  assert.equal(call(poll).status,'timeout');fs.writeFileSync(cachePath,png);
  listener.onRichMediaDownloadComplete(complete({filePath:cachePath}));
  assert.equal(fs.existsSync(path.dirname(lateTarget)),false);
  assert.equal(fs.existsSync(cachePath),true,'late cache completion must preserve the native file');
  invalidCache=true;const beforeInvalid=calls;await begin();assert.equal(call(poll).status,'invalid_response');
  assert.equal(calls,beforeInvalid,'invalid registered path must not dispatch');invalidCache=false;
  mode='pending';await begin();
  const outside=path.join(cacheRoot,'unregistered.png');fs.writeFileSync(outside,png);
  listener.onRichMediaDownloadComplete(complete({filePath:outside}));assert.equal(call(poll).status,'pending');
  finish();assert.equal(call(poll).status,'success');assert.equal(fs.existsSync(outside),true);
  senderUin='20002';mode='pending';await begin();const retained=requested;
  assert.ok(eval(stop).startsWith('STOPPED'));assert.equal(fs.existsSync(path.dirname(retained)),true);
  finish();assert.equal(fs.existsSync(path.dirname(retained)),false);assert.equal(call(start).status,'not_ready');
  fs.rmSync(cacheRoot,{recursive:true,force:true});
  process.stdout.write(JSON.stringify(success));
})().catch(e=>{console.error(e);process.exitCode=1;});
"#;
    let json = run_node(&script);
    assert!(!json.contains("filePath"));
    let QueryOutcome::Success(QueryData::ImageDownload(download)) =
        qq_query::decode_query_result(&q, &scope(), &json).unwrap()
    else {
        panic!("owned PNG missing")
    };
    assert_eq!(download.png_hex.len(), download.info.size as usize * 2);
    let mut wrong = download;
    wrong.info.width = 2;
    assert!(QueryData::ImageDownload(wrong)
        .validate_for(&q, &scope())
        .is_err());
}
