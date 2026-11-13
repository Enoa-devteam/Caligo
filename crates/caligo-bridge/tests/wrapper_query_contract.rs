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
    let output = std::process::Command::new("node")
        .args(["--unhandled-rejections=strict", "-e", &script])
        .output()
        .unwrap();
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

reset({{getBuddyService() {{ return {{getBuddyListV2(a,b,c) {{ assert.deepEqual([a,b,c], ['0',true,0]); return Promise.resolve({{data:[{{buddyUids:['u_friend']}}]}}); }}}}; }}, getProfileService() {{ return {{getCoreAndBaseInfo(source,uids) {{ assert.equal(source,'nodeStore'); assert.deepEqual(uids,['u_friend']); return Promise.resolve(new Map([['u_friend',{{coreInfo:{{uin:'20002',nick:'Friend',remark:''}}}}]])); }}}}; }}}});
assert.equal(json(START_FRIEND).status, 'pending'); await tick(); await tick();
assert.deepEqual(json(POLL_FRIEND), {{status:'success',data:{{kind:'friend_list',data:[{{user_id:'20002',nickname:'Friend',remark:''}}]}}}});

let listener;
reset({{getGroupService() {{ return {{addKernelGroupListener(x) {{ listener=x; return 11; }}, getGroupList() {{ return Promise.resolve({{result:0}}); }}}}; }}}});
assert.equal(json(START_GROUPS).status, 'pending');
assert.equal(json(START_GROUPS).code, 409);
listener.onGroupListUpdate(2, [{{groupCode:'bad',groupName:'bad',memberCount:1,maxMember:2}}]);
assert.equal(json(POLL_GROUPS).status, 'pending');
listener.onGroupListUpdate(0, [{{groupCode:'20002',groupName:'Group',memberCount:1,maxMember:200}}]);
assert.deepEqual(json(POLL_GROUPS), {{status:'success',data:{{kind:'group_list',data:[{{group_id:'20002',group_name:'Group',member_count:1,max_member_count:200}}]}}}});

reset({{getGroupService() {{ return {{getGroupDetailInfo(code, source) {{ assert.equal(code,'20002'); assert.equal(source,2); return Promise.resolve({{result:0,groupCode:'other',groupName:'x',memberNum:1,maxMemberNum:2}}); }}}}; }}}});
assert.equal(json(START_DETAIL).status, 'pending'); await tick();
assert.equal(json(POLL_DETAIL).status, 'invalid_response');

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
