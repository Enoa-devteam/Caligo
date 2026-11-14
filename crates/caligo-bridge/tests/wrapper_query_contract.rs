#[path = "../src/qq_query.rs"]
mod qq_query;

use caligo_model::capability::{HostQuery, QueryScope};

fn scope() -> QueryScope {
    QueryScope {
        account: "10001".into(),
        host_nonce: 7,
        session_generation: 8,
        connection_epoch: 9,
    }
}

#[test]
fn voice_readback_requires_exact_private_incoming_element_and_copies_only_metadata() {
    use caligo_model::capability::{QueryData, QueryOutcome, VoiceInfo};

    let scope = scope();
    let query = HostQuery::VoiceInfo {
        peer_uid: "u_voice_peer".into(),
        message_id: "90071992547409939999".into(),
        element_id: "9007199254740994".into(),
    };
    assert!(query.validate().is_ok());
    assert!(!qq_query::is_supported(&query));
    let start = qq_query::build_query_start_js("voice", &scope, &query, 1000);
    let poll = qq_query::build_query_poll_js("voice", &scope);
    let script = format!(
        "const start={}, poll={};\n",
        serde_json::to_string(&start).unwrap(),
        serde_json::to_string(&poll).unwrap()
    ) + r#"
const assert = require('node:assert/strict');
const md5 = '0123456789ABCDEF0123456789ABCDEF';
const msgId = '90071992547409939999', elementId = '9007199254740994';
function fresh() {
  return {msgId,chatType:1,peerUid:'u_voice_peer',senderUid:'u_voice_peer',senderUin:'20002',
    elements:[{elementId,elementType:4,pttElement:{fileSize:'1073',md5HexStr:md5,filePath:'private',fileUuid:'private'}}]};
}

async function read(record) {
  const service = {getMsgsByMsgId(peer, ids) {
    assert.equal(this,service);
    assert.deepEqual(peer,{chatType:1,peerUid:'u_voice_peer',guildId:''});
    assert.deepEqual(ids,[msgId]);
    return Promise.resolve({result:0,msgList:[record]});
  }};
  globalThis.__caligo_d8 = {sid:'nt_1',session:{getMsgService:()=>service}};
  assert.equal(JSON.parse(eval(start)).status,'pending');
  await new Promise(resolve => setImmediate(resolve));
  const result = JSON.parse(eval(poll));
  assert.equal(JSON.parse(eval(poll)).code,404);
  return result;
}
(async () => {
  for (const mutate of [
    m=>m.msgId='999', m=>m.chatType=2, m=>m.peerUid='u_wrong', m=>m.senderUid='u_self',
    m=>m.senderUin='10001', m=>m.elements[0].elementId=9007199254740994,
    m=>m.elements.push(m.elements[0]), m=>m.elements[0].elementType=1,
    m=>m.elements[0].pttElement.fileSize='16777217',
    m=>m.elements[0].pttElement.md5HexStr='bad',
  ]) {
    const message=fresh(); mutate(message);
    assert.equal((await read(message)).status,'invalid_response');
  }
  process.stdout.write(JSON.stringify(await read(fresh())));
})().catch(error => { console.error(error); process.exitCode=1; });
"#;
    use std::io::Write;
    let mut child = std::process::Command::new("node")
        .args(["--unhandled-rejections=strict", "-"])
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body = String::from_utf8(output.stdout).unwrap();
    assert!(!body.contains("private"));
    let expected = VoiceInfo {
        peer_uid: "u_voice_peer".into(),
        message_id: "90071992547409939999".into(),
        element_id: "9007199254740994".into(),
        size: 1073,
        md5: "0123456789abcdef0123456789abcdef".into(),
    };
    assert_eq!(
        qq_query::decode_query_result(&query, &scope, &body).unwrap(),
        QueryOutcome::Success(QueryData::VoiceInfo(expected.clone()))
    );
    let mut wrong = expected;
    wrong.element_id = "999".into();
    assert!(QueryData::VoiceInfo(wrong)
        .validate_for(&query, &scope)
        .is_err());
}

#[cfg(feature = "research")]
#[test]
fn voice_download_waits_for_correlated_callback_and_verifies_owned_file() {
    use caligo_bridge::qq_v8::{D8_POLL_JS, D8_START_JS, D8_STOP_JS};
    use caligo_model::capability::{QueryData, QueryOutcome};
    use std::io::Write;
    let scope = scope();
    let query = HostQuery::VoiceDownload {
        peer_uid: "u_voice_peer".into(),
        message_id: "90071992547409939999".into(),
        element_id: "9007199254740994".into(),
    };
    let quote = |s: &str| serde_json::to_string(s).unwrap();
    let exact = |chat_type, peer_uid: &str, sender_uin: &str| {
        qq_query::build_query_start_js("voice", &scope, &HostQuery::ExactVoiceDownload {
            chat_type, peer_uid: peer_uid.into(), sender_uin: sender_uin.into(),
            message_id: "90071992547409939999".into(), element_id: "9007199254740994".into(),
        }, 1000)
    };
    let script = format!(
        "const arm={}, drain={}, stop={}, start={}, poll={}, cancel={}, group={}, sent={};\n",
        quote(D8_START_JS),
        quote(D8_POLL_JS),
        quote(D8_STOP_JS),
        quote(&qq_query::build_profile_query_start_js(
            "voice", &scope, &query, 1000, true
        )),
        quote(&qq_query::build_query_poll_js("voice", &scope)),
        quote(&qq_query::build_query_cancel_js("voice", &scope)),
        quote(&exact(2, "30003", "20002")),
        quote(&exact(1, "u_voice_peer", "10001")),
    ) + r#"
const assert = require('node:assert/strict');
const fs = require('node:fs');
const crypto = require('node:crypto');
const path = require('node:path');
const os = require('node:os');
const silk = Buffer.from('02232153494c4b5f563300ff','hex');
const md5 = crypto.createHash('md5').update(silk).digest('hex');
const msgId='90071992547409939999', elementId='9007199254740994';
let listener, requested, calls = 0, delayed, mode = 'pending';
let chatType=1, peerUid='u_voice_peer', peerUin='20002', senderUin='20002';
let voiceCachePath;
const service = {
  sendMsg() {},
  addKernelMsgListener(value) { listener=value; return 7; },
  removeKernelMsgListener(id) { assert.equal(id,7); },
  getMsgsByMsgId(peer, ids) {
    assert.equal(this,service);
    assert.deepEqual(peer,{chatType,peerUid,guildId:''});
    assert.deepEqual(ids,[msgId]);
    const result = {result:0,msgList:[{msgId,chatType,peerUid,peerUin,
      senderUid:'u_voice_peer',senderUin,sendStatus:2,elements:[{elementId,elementType:4,
      pttElement:{fileSize:String(silk.length),md5HexStr:md5,filePath:voiceCachePath}}]}]};
    if (mode === 'delayed') return new Promise(resolve => delayed=()=>resolve(result));
    return Promise.resolve(result);
  },
  downloadRichMedia(args) {
    assert.equal(this,service);
    calls++; requested = args.filePath;
    assert.ok(path.isAbsolute(requested));
    assert.deepEqual(args,{msgId,elementId,peerUid,chatType,fileModelId:'0',
      downSourceType:0,downloadSourceType:0,triggerType:1,thumbSize:0,downloadType:1,filePath:requested});
    if (mode === 'sync') {
      fs.writeFileSync(requested,silk);
      listener.onRichMediaDownloadComplete(completion());
    }
    return Promise.resolve({result:0});
  },
};
function completion(overrides={}) {
  return {msgId,msgElementId:elementId,chatType,fileErrCode:'0',filePath:requested,...overrides};
}
const session = {getSessionId:()=>1,getMsgService:()=>service};
process._linkedBinding = () => ({NodeIQQNTWrapperSession:{getNTWrapperSession:()=>session}});
assert.ok(eval(arm).startsWith('ARMED'));
const call = source => JSON.parse(eval(source));
const tick = () => new Promise(resolve => setImmediate(resolve));
(async () => {
  mode='delayed';
  assert.equal(call(start).status,'pending');
  assert.equal(call(cancel).status,'cancelled');
  delayed(); await tick();
  assert.equal(calls,0,'a late readback may not start a download');
  assert.deepEqual(call(drain).mediaDiagnostics,[{media:'voice',stage:'readback'}]);

  mode='pending';
  assert.equal(call(start).status,'pending'); await tick();
  assert.equal(calls,1);
  listener.onRichMediaDownloadComplete(completion({msgElementId:'123'}));
  listener.onRichMediaDownloadComplete(completion({filePath:requested+'.other',chatType:null}));
  assert.equal(call(poll).status,'pending','Promise acceptance and unmatched callbacks do not complete');
  const diagnostics=call(drain).mediaDiagnostics;
  assert.deepEqual(diagnostics.map(row=>row.stage),
    ['readback','readback_matched','dispatch','promise','callback','callback']);
  assert.ok(diagnostics.every(row=>row.media==='voice'));
  assert.equal(diagnostics[3].result,'accepted');
  assert.equal(diagnostics[4].element_matches,false);
  assert.equal(diagnostics[5].element_matches,true);
  assert.equal(diagnostics[5].callback_chat,'null');
  assert.equal(diagnostics[5].registered_path_matches,false);
  assert.ok(!JSON.stringify(diagnostics).includes(requested));
  assert.equal(call(cancel).status,'cancelled');
  assert.equal(call(start).code,409,'native callback is still outstanding');
  const oldPath=requested;
  fs.writeFileSync(oldPath,silk);
  listener.onRichMediaDownloadComplete(completion());
  assert.equal(call(drain).mediaDiagnostics[0].pending,false);
  assert.equal(fs.existsSync(path.dirname(oldPath)),false);

  assert.equal(call(start).status,'pending'); await tick();
  assert.notEqual(requested,oldPath);
  listener.onRichMediaDownloadComplete(completion({filePath:oldPath}));
  fs.writeFileSync(requested,Buffer.alloc(silk.length,1));
  listener.onRichMediaDownloadComplete(completion());
  assert.equal(call(poll).status,'invalid_response');
  assert.ok(call(drain).mediaDiagnostics.some(row=>row.stage==='validation_failed'));

  mode='sync';
  assert.equal(call(start).status,'pending'); await tick();
  const success=call(poll);
  assert.equal(fs.existsSync(path.dirname(requested)),false);
  assert.equal(call(poll).code,404);
  assert.equal(calls,3);
  assert.ok(call(drain).mediaDiagnostics.some(row=>row.stage==='validated'));
  chatType=2; peerUid=peerUin='30003'; senderUin='20099';
  assert.equal(call(group).status,'pending'); await tick();
  assert.equal(call(poll).status,'invalid_response');
  assert.equal(calls,3);
  senderUin='20002'; mode='pending';
  assert.equal(call(group).status,'pending'); await tick();
  assert.equal(calls,4);
  fs.writeFileSync(requested,silk);
  listener.onRichMediaDownloadComplete(completion({chatType:1}));
  assert.equal(call(poll).status,'pending');
  listener.onRichMediaDownloadComplete(completion());
  assert.equal(call(poll).data.data.info.peer_uid,'30003');
  chatType=1; peerUid='u_voice_peer'; peerUin='20002'; senderUin='10001'; mode='sync';
  assert.equal(call(sent).status,'pending'); await tick();
  assert.equal(call(poll).status,'success');
  assert.equal(calls,5);
  assert.deepEqual(call(drain).mediaDiagnostics,[],'voice diagnostics require the verified pilot');
  senderUin='20002';

  const cacheRoot=fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()),'voice-cache-test-'));
  const cachePath=account=>path.join(cacheRoot,account,'nt_qq','nt_data','Ptt','2026-11','Ori',md5+'.amr');
  const ownCache=cachePath('10001');
  const otherCache=cachePath('10002');
  try {
    for (const target of [ownCache,otherCache]) {
      fs.mkdirSync(path.dirname(target),{recursive:true});
      fs.writeFileSync(target,silk);
    }
    mode='pending';
    voiceCachePath=otherCache;
    assert.equal(call(start).status,'pending'); await tick();
    listener.onRichMediaDownloadComplete(completion({chatType:null,filePath:otherCache}));
    assert.equal(call(poll).status,'pending','another account cache must not qualify');
    fs.writeFileSync(requested,silk);
    listener.onRichMediaDownloadComplete(completion());
    assert.equal(call(poll).status,'success');
    call(drain);

    voiceCachePath=ownCache;
    assert.equal(call(start).status,'pending'); await tick();
    for (const extra of [
      {filePath:otherCache}, {filePath:requested}, {msgElementId:'123'},
      {chatType:undefined}, {chatType:'1'}, {fileErrCode:'5'},
    ]) {
      listener.onRichMediaDownloadComplete(completion({chatType:null,filePath:ownCache,...extra}));
      assert.equal(call(poll).status,'pending');
    }
    call(drain);
    listener.onRichMediaDownloadComplete(completion({chatType:null,filePath:ownCache}));
    assert.equal(call(poll).data.data.silk_hex,silk.toString('hex'));
    assert.deepEqual(fs.readFileSync(ownCache),silk);
    assert.equal(fs.existsSync(path.dirname(requested)),false);

    assert.equal(call(start).status,'pending'); await tick();
    fs.writeFileSync(ownCache,Buffer.alloc(silk.length));
    listener.onRichMediaDownloadComplete(completion({chatType:null,filePath:ownCache}));
    assert.equal(call(poll).status,'invalid_response');
    assert.equal(fs.existsSync(ownCache),true);
  } finally {
    fs.rmSync(cacheRoot,{recursive:true,force:true});
  }
  voiceCachePath=undefined;
  const callsBeforeStop=calls;
  mode='delayed';
  assert.equal(call(start).status,'pending');
  assert.ok(eval(stop).startsWith('STOPPED'));
  delayed(); await tick();
  assert.equal(calls,callsBeforeStop,'stop prevents a late readback from dispatching');
  assert.equal(call(start).status,'not_ready');
  process.stdout.write(JSON.stringify(success));
})().catch(error => { console.error(error); process.exitCode=1; });
"#;
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
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(!json.contains("filePath"));
    let QueryOutcome::Success(QueryData::VoiceDownload(result)) =
        qq_query::decode_query_result(&query, &scope, &json).unwrap()
    else {
        panic!("expected owned voice bytes");
    };
    assert_eq!(result.silk_hex, "02232153494c4b5f563300ff");
    assert_eq!(result.info.size, 12);
}
#[test]
fn fixed_scripts_cover_maps_messages_directories_and_lifecycle_edges() {
    let s = scope();
    let start_uin = qq_query::build_query_start_js(
        "early",
        &s,
        &HostQuery::UinToUid {
            uin: "90001".into(),
        },
        1000,
    );
    let start_uid = qq_query::build_query_start_js(
        "late",
        &s,
        &HostQuery::UidToUin {
            uid: "u_friend".into(),
        },
        1000,
    );
    let start_msg = qq_query::build_query_start_js(
        "msg",
        &s,
        &HostQuery::MessagesById {
            chat_type: 2,
            peer_uid: "20002".into(),
            message_ids: vec!["9".into()],
        },
        1000,
    );
    let start_friend = qq_query::build_query_start_js("friend", &s, &HostQuery::FriendList, 1000);
    let start_groups = qq_query::build_query_start_js("groups", &s, &HostQuery::GroupList, 1000);
    let start_pilot_groups = qq_query::build_profile_query_start_js("groups", &s, &HostQuery::GroupList, 1000, true);
    let start_detail = qq_query::build_query_start_js(
        "detail",
        &s,
        &HostQuery::GroupInfo {
            group_id: "20002".into(),
        },
        1000,
    );
    let poll = |id: &str| qq_query::build_query_poll_js(id, &s);
    let cancel = qq_query::build_query_cancel_js("late-cancel", &s);

    fn js_string(s: &str) -> String {
        serde_json::to_string(s).unwrap()
    }
    let script = format!(
        r#"
const assert = require('node:assert/strict');
const vm = require('node:vm');
(async function() {{
const START_UIN = {start_uin};
const START_UID = {start_uid};
const START_MSG = {start_msg};
const START_FRIEND = {start_friend};
const START_GROUPS = {start_groups};
const START_PILOT_GROUPS = {start_pilot_groups};
const START_DETAIL = {start_detail};
const POLL_EARLY = {poll_early};
const POLL_LATE = {poll_late};
const POLL_MSG = {poll_msg};
const POLL_FRIEND = {poll_friend};
const POLL_GROUPS = {poll_groups};
const POLL_DETAIL = {poll_detail};
const POLL_MISSING = {poll_missing};
const CANCEL = {cancel};
const SCOPE_TEXT = {scope_text};
function run(source) {{ return eval(source); }}
function json(source) {{ return JSON.parse(run(source)); }}
function reset(session) {{ globalThis.__caligo_d8 = {{sid:'nt_0', session, queryPending:new Map()}}; }}
async function tick() {{ await new Promise(resolve => setImmediate(resolve)); }}

reset({{getProfileService() {{ return {{getUidByUin(source, keys) {{ assert.equal(source, 'FriendsServiceImpl'); assert.deepEqual(keys, ['90001']); return new Map([['90001','u_friend']]); }}}}; }}}});
assert.equal(json(START_UIN).status, 'pending'); await tick();
assert.deepEqual(json(POLL_EARLY), {{status:'success',data:{{kind:'uid',data:'u_friend'}}}});
assert.equal(json(POLL_MISSING).code, 404);

reset({{getProfileService() {{ return {{getUinByUid() {{ return Promise.resolve(new Map([['u_friend','90001']])); }}}}; }}}});
assert.equal(json(START_UID).status, 'pending'); assert.equal(json(POLL_LATE).status, 'pending'); await tick();
assert.deepEqual(json(POLL_LATE), {{status:'success',data:{{kind:'uin',data:'90001'}}}});

reset({{getMsgService() {{ return {{getMsgsByMsgId(peer, ids) {{ assert.deepEqual(peer, {{chatType:2,peerUid:'20002'}}); assert.deepEqual(ids, ['9']); return Promise.resolve({{msgList:[]}}); }}}}; }}}});
assert.equal(json(START_MSG).status, 'pending'); await tick();
assert.equal(json(POLL_MSG).status, 'success');

reset({{getBuddyService() {{ return {{getBuddyListV2(a,b,c) {{ assert.deepEqual([a,b,c], ['0',true,0]); return Promise.resolve({{data:[{{buddyUids:['u_friend']}},{{buddyUids:['u_friend']}}]}}); }}}}; }}, getProfileService() {{ return {{getCoreAndBaseInfo(source,uids) {{ assert.equal(source,'nodeStore'); assert.deepEqual(uids,['u_friend']); return Promise.resolve(new Map([['u_friend',{{coreInfo:{{uin:'20002',nick:'Friend',remark:''}}}}]])); }}}}; }}}});
assert.equal(json(START_FRIEND).status, 'pending'); await tick(); await tick();
assert.deepEqual(json(POLL_FRIEND), {{status:'success',data:{{kind:'friend_list',data:[{{user_id:'20002',nickname:'Friend',remark:''}}]}}}});
for (const [buddyUids, reason] of [
  [[{{private:'do not log'}}], 'buddy uid type'],
  [Array.from({{length:2049}}, (_, i) => 'u_' + i), 'buddy list over budget'],
]) {{
  reset({{getBuddyService() {{ return {{getBuddyListV2() {{ return Promise.resolve({{data:[{{buddyUids}}]}}); }}}}; }}, getProfileService() {{ return {{getCoreAndBaseInfo() {{ assert.fail('invalid list must not reach ProfileService'); }}}}; }}}});
  assert.equal(json(START_FRIEND).status, 'pending'); await tick();
  const invalid = json(POLL_FRIEND);
  assert.equal(invalid.reason, 'Error: ' + reason);
  assert(!JSON.stringify(invalid).includes('do not log'));
}}

let listener;
reset({{getGroupService() {{ return {{addKernelGroupListener(x) {{ listener=x; return 11; }}, getGroupList() {{ return Promise.resolve({{result:0}}); }}}}; }}}});
assert.equal(json(START_GROUPS).status, 'pending');
assert.equal(json(START_GROUPS).code, 409);
listener.onGroupListUpdate(2, [{{groupCode:'bad',groupName:'bad',memberCount:1,maxMember:2}}]);
assert.equal(json(POLL_GROUPS).status, 'pending');
listener.onGroupListUpdate(0, [{{groupCode:'20002',groupName:'Group',memberCount:1,maxMember:200}}]);
assert.deepEqual(json(POLL_GROUPS), {{status:'success',data:{{kind:'group_list',data:[{{group_id:'20002',group_name:'Group',member_count:1,max_member_count:200}}]}}}});
const goodGroup = {{groupCode:'20002',groupName:'Group',memberCount:1,maxMember:200}};
for (const [groups, reason] of [
  [[goodGroup, goodGroup], 'duplicate group id'],
  [[{{...goodGroup,groupCode:'0'}}], 'invalid group id'],
  [[{{...goodGroup,memberCount:201}}], 'group member count exceeds capacity'],
  [[{{...goodGroup,maxMember:0}}], 'group capacity zero with members'],
  [[{{...goodGroup,maxMember:4294967296}}], 'group capacity out of range'],
  [[{{...goodGroup,memberCount:-1}}], 'group member count out of range'],
]) {{
  assert.equal(json(START_GROUPS).status, 'pending');
  listener.onGroupListUpdate(0, groups);
  assert.equal(json(POLL_GROUPS).reason, 'Error: ' + reason);
}}
assert.equal(json(START_PILOT_GROUPS).status, 'pending');
listener.onGroupListUpdate(0, [{{...goodGroup,memberCount:201}}]);
assert.deepEqual(json(POLL_GROUPS).data.data, [{{group_id:'20002',group_name:'Group',member_count:201,max_member_count:200}}]);
assert.equal(json(START_PILOT_GROUPS).status, 'pending');
listener.onGroupListUpdate(0, [{{...goodGroup,maxMember:0}}]);
assert.equal(json(POLL_GROUPS).reason, 'Error: group capacity zero with members');

let detailListener;
reset({{getGroupService() {{ return {{removeKernelGroupListener() {{}}, addKernelGroupListener(x) {{ detailListener=x; return 12; }}, getGroupDetailInfo(code, source) {{ assert.equal(code,'20002'); assert.equal(source,2); return Promise.resolve({{result:0}}); }}}}; }}}});
assert.equal(json(START_DETAIL).status, 'pending'); await tick();
detailListener.onGroupDetailInfoChange({{groupCode:'20002',groupName:'Group',memberNum:3,maxMemberNum:200}});
assert.deepEqual(json(POLL_DETAIL), {{status:'success',data:{{kind:'group_info',data:{{group_id:'20002',group_name:'Group',member_count:3,max_member_count:200}}}}}});
reset({{getGroupService() {{ return {{removeKernelGroupListener() {{}}, addKernelGroupListener(x) {{ detailListener=x; return 13; }}, getGroupDetailInfo(code) {{ return Promise.resolve({{result:0}}); }}}}; }}}});
assert.equal(json(START_DETAIL).status, 'pending'); await tick();
detailListener.onGroupDetailInfoChange({{groupCode:'other',groupName:'Wrong',memberNum:1,maxMemberNum:2}});
assert.equal(json(POLL_DETAIL).status, 'pending');
detailListener.onGroupDetailInfoChange({{groupCode:'20002',groupName:'Group 2',memberNum:4,maxMemberNum:200}});
assert.deepEqual(json(POLL_DETAIL), {{status:'success',data:{{kind:'group_info',data:{{group_id:'20002',group_name:'Group 2',member_count:4,max_member_count:200}}}}}});
reset({{getGroupService() {{ return {{removeKernelGroupListener() {{}}, addKernelGroupListener(x) {{ detailListener=x; return 13; }}, getGroupDetailInfo() {{ return Promise.resolve({{result:7}}); }}}}; }}}});
assert.equal(json(START_DETAIL).status, 'pending'); await tick();
assert.equal(json(POLL_DETAIL).status, 'rejected');

reset({{getProfileService() {{ return {{getUidByUin() {{ return new Promise(() => {{}}); }}}}; }}}});
const timeoutStart = {timeout_start}; const timeoutPoll = {timeout_poll};
assert.equal(json(timeoutStart).status, 'pending'); await new Promise(resolve => setTimeout(resolve, 15));
assert.equal(json(timeoutPoll).status, 'timeout');

reset({{getProfileService() {{ return {{getUidByUin() {{ return new Map(); }}}}; }}}});
const cancelStart = {cancel_start};
assert.equal(json(cancelStart).status, 'pending');
assert.equal(json(CANCEL).status, 'cancelled');

const wrongScope = JSON.parse(SCOPE_TEXT); wrongScope.connection_epoch = '99';
const wrongPoll = {wrong_poll};
assert.equal(json(wrongPoll).code, 409);
assert.equal(START_UIN.includes('eval('), false);
}})().catch(error => {{ console.error(error); process.exitCode = 1; }});
"#,
        start_uin = js_string(&start_uin),
        start_uid = js_string(&start_uid),
        start_msg = js_string(&start_msg),
        start_friend = js_string(&start_friend),
        start_groups = js_string(&start_groups),
        start_pilot_groups = js_string(&start_pilot_groups),
        start_detail = js_string(&start_detail),
        poll_early = js_string(&poll("early")),
        poll_late = js_string(&poll("late")),
        poll_msg = js_string(&poll("msg")),
        poll_friend = js_string(&poll("friend")),
        poll_groups = js_string(&poll("groups")),
        poll_detail = js_string(&poll("detail")),
        poll_missing = js_string(&poll("missing")),
        cancel = js_string(&cancel),
        scope_text = js_string(&serde_json::to_string(&s).unwrap()),
        timeout_start = js_string(&qq_query::build_query_start_js(
            "timeout",
            &s,
            &HostQuery::UinToUid {
                uin: "90001".into()
            },
            1
        )),
        timeout_poll = js_string(&poll("timeout")),
        cancel_start = js_string(&qq_query::build_query_start_js(
            "late-cancel",
            &s,
            &HostQuery::UinToUid {
                uin: "90001".into()
            },
            1000
        )),
        wrong_poll = js_string(&qq_query::build_query_poll_js(
            "early",
            &QueryScope {
                connection_epoch: 99,
                ..s.clone()
            }
        )),
    );
    let path =
        std::env::temp_dir().join(format!("caligo-wrapper-query-{}.cjs", std::process::id()));
    std::fs::write(&path, script).expect("write wrapper query Node fixture");
    let output = std::process::Command::new("node")
        .args(["--unhandled-rejections=strict", path.to_str().unwrap()])
        .output()
        .expect("Node.js is required for wrapper query contract");
    let _ = std::fs::remove_file(&path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn group_detail_joins_acceptance_and_notification_and_isolates_late_callbacks() {
    use std::io::Write;
    let s = scope();
    let detail = |group: &str| HostQuery::GroupInfo { group_id: group.into() };
    let scripts = serde_json::json!({
        "start": qq_query::build_query_start_js("detail", &s, &detail("30003"), 5000),
        "pilotStart": qq_query::build_profile_query_start_js("detail", &s, &detail("30003"), 5000, true),
        "poll": qq_query::build_query_poll_js("detail", &s),
        "cancel": qq_query::build_query_cancel_js("detail", &s),
        "next": qq_query::build_query_start_js("next", &s, &detail("30004"), 5000),
        "pilotNext": qq_query::build_profile_query_start_js("next", &s, &detail("30004"), 5000, true),
        "nextPoll": qq_query::build_query_poll_js("next", &s),
        "rebind": qq_query::build_query_start_js("new-epoch", &QueryScope { connection_epoch: 10, ..s.clone() }, &detail("30003"), 5000),
    });
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/group-detail-contract.cjs");
    let mut child = std::process::Command::new("node").arg(fixture)
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(scripts.to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn group_members_require_complete_map_snapshot_and_preserve_native_identity() {
    use caligo_model::capability::{QueryData, QueryOutcome};
    let s = scope();
    let query = HostQuery::GroupMembers {
        group_id: "30003".into(),
        force_refresh: true,
    };
    assert!(
        !qq_query::is_supported(&query),
        "new native contract requires exact pilot"
    );
    let start = qq_query::build_query_start_js("members", &s, &query, 1000);
    let pilot_start = qq_query::build_profile_query_start_js("members", &s, &query, 1000, true);
    let poll = qq_query::build_query_poll_js("members", &s);
    let script = format!(
        "const start={},pilotStart={},poll={};\n",
        serde_json::to_string(&start).unwrap(),
        serde_json::to_string(&pilot_start).unwrap(),
        serde_json::to_string(&poll).unwrap()
    ) + r#"
const assert=require('node:assert/strict');
function member(){return {uid:'u_member',uin:'18446744073709551615',nick:'Member',cardName:'Card',role:4,
  memberRealLevel:12,qqLevel:{crownNum:1,sunNum:2,moonNum:3,starNum:1},sex:2,age:27,
  joinTime:'1700000000',lastSpeakTime:'1700000001',memberSpecialTitle:'Title',isDelete:false,isRobot:false,shutUpTime:0,
  avatarPath:'private-path',credential:'private-secret'};}
function fresh(){return {errCode:0,errMsg:'',result:{ids:[{uid:'u_member',index:0}],infos:new Map([['u_member',member()]]),finish:true,hasRobot:false}};}
async function run(value, script=start){
 const service={getAllMemberList(group,force){assert.equal(this,service);assert.equal(group,'30003');assert.equal(force,true);return Promise.resolve(value);}};
 globalThis.__caligo_d8={sid:'nt_members',session:{getGroupService:()=>service}};
 assert.equal(JSON.parse(eval(script)).status,'pending');await new Promise(resolve=>setImmediate(resolve));
 return JSON.parse(eval(poll));
}
(async()=>{
 for(const mutate of [
   v=>v.result.finish=false,v=>v.result.infos=[],v=>v.result.ids=[],
   v=>v.result.ids.push(v.result.ids[0]),v=>v.result.infos.set('u_other',member()),
   v=>v.result.infos.get('u_member').uid='u_wrong',v=>v.result.infos.get('u_member').uin=123,
   v=>v.result.infos.get('u_member').role=99,v=>v.result.infos.get('u_member').joinTime='18446744073709551616',
   v=>v.result.infos.get('u_member').isDelete=true,v=>v.result.infos.get('u_member').nick='a'.repeat(4097),
 ]){const v=fresh();mutate(v);assert.equal((await run(v)).status,'invalid_response');}
 let rejected=fresh();rejected.errCode=5;assert.equal((await run(rejected)).status,'rejected');
 let empty=fresh();empty.result.ids=[];empty.result.infos=new Map();assert.deepEqual((await run(empty)).data.data.members,[]);
 const observed=fresh();const item=observed.result.infos.get('u_member');
 item.joinTime=1700000000;item.lastSpeakTime=0;
 assert.equal((await run(observed)).status,'invalid_response');
 for(const invalid of [-1, 1.5, NaN, 9007199254740992, null]) {
   item.joinTime=invalid;
   assert.equal((await run(observed,pilotStart)).reason,'Error: invalid member timestamp');
 }
 item.joinTime=1700000000;
 assert.equal((await run(fresh(),pilotStart)).status,'success');
 const valid=await run(observed,pilotStart);assert.equal(valid.status,'success');
 assert.equal(valid.data.data.members[0].join_time,'1700000000');
 assert.equal(valid.data.data.members[0].last_sent_time,'0');
 assert.equal(valid.data.data.members[0].user_id,'18446744073709551615');assert.equal(valid.data.data.members[0].role,'owner');
 assert.equal(valid.data.data.members[0].qq_level,109);assert.equal(valid.data.data.members[0].sex,'female');
 const raw=JSON.stringify(valid);assert(!raw.includes('private-path'));assert(!raw.includes('private-secret'));assert(!raw.includes('u_member'));
 process.stdout.write(raw);
})().catch(error=>{console.error(error);process.exit(1)});
"#;
    let path = std::env::temp_dir().join(format!("caligo-members-{}.cjs", std::process::id()));
    std::fs::write(&path, script).unwrap();
    let output = std::process::Command::new("node")
        .arg(&path)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result =
        qq_query::decode_query_result(&query, &s, std::str::from_utf8(&output.stdout).unwrap())
            .unwrap();
    let QueryOutcome::Success(QueryData::GroupMembers(list)) = result else {
        panic!("members not decoded")
    };
    assert_eq!(list.group_id, "30003");
    assert_eq!(list.members[0].user_id, u64::MAX.to_string());
}
