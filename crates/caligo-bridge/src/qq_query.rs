//! Fixed read-only wrapper query executor scripts.
//!
//! The scripts in this module are a closed enum of wrapper calls. The caller
//! executes them on the QQ owner thread through `qq_v8::exec_script`.

use caligo_model::capability::{
    HostQuery, QueryCapability, QueryData, QueryOutcome, QueryScope, MAX_QUERY_BYTES,
    MAX_QUERY_TIMEOUT_MS, MAX_VOICE_BYTES,
};
use serde_json::Value;

/// Capabilities with an implemented and bounded wrapper contract.
/// LoginInfo is intentionally absent until a verified current-account source
/// is available from the wrapper session.
pub const SUPPORTED_CAPABILITIES: &[QueryCapability] = &[
    QueryCapability::UinToUid,
    QueryCapability::UidToUin,
    QueryCapability::MessagesById,
    QueryCapability::FriendList,
    QueryCapability::GroupList,
    QueryCapability::GroupInfo,
    QueryCapability::VoiceInfo,
];

pub fn supported_capabilities() -> &'static [QueryCapability] {
    // Voice operations form a suffix admitted only by the exact pilot profile.
    &SUPPORTED_CAPABILITIES[..SUPPORTED_CAPABILITIES.len() - 1]
}

pub fn is_supported(query: &HostQuery) -> bool {
    supported_capabilities().contains(&query.capability())
}

/// Verify the paths of the modules actually mapped in the current process.
/// A caller-provided baseline label or installed version is insufficient.
#[cfg(feature = "research")]
pub(crate) fn matches_voice_profile(loaded_modules: &[String]) -> bool {
    use sha2::{Digest, Sha256};
    use std::path::Path;

    const MODULES: [(&str, &str); 3] = [
        (
            "wrapper.node",
            "0D8B94448F4DAFE7337B88D6A568B004224C37FCBE2BD1142C1AF7624FBD534C",
        ),
        (
            "QQNT.dll",
            "75B19E58A123B0513E99DF4DF3EBC6524478535BA44651024D6A20821F5086E5",
        ),
        (
            "major.node",
            "4564E402D1CF43A0F15982411596CF38B8D9B8FBC78F5E7CA717E27C24C74A07",
        ),
    ];
    MODULES.into_iter().all(|(name, expected)| {
        let mut paths = loaded_modules.iter().map(Path::new).filter(|path| {
            path.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case(name))
        });
        let Some(path) = paths.next() else {
            return false;
        };
        if paths.next().is_some() {
            return false;
        }
        let Ok(mut file) = std::fs::File::open(path) else {
            return false;
        };
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher).is_ok()
            && format!("{:X}", hasher.finalize()) == expected
    })
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("JSON string serialization is infallible")
}

fn kind(query: &HostQuery) -> &'static str {
    match query {
        HostQuery::LoginInfo => "login_info",
        HostQuery::FriendList => "friend_list",
        HostQuery::GroupList => "group_list",
        HostQuery::GroupInfo { .. } => "group_info",
        HostQuery::UinToUid { .. } => "uin_to_uid",
        HostQuery::UidToUin { .. } => "uid_to_uin",
        HostQuery::MessagesById { .. } => "messages_by_id",
        HostQuery::VoiceInfo { .. } => "voice_info",
    }
}

fn query_args(query: &HostQuery) -> String {
    match query {
        HostQuery::UinToUid { uin } => format!("{{uin:{}}}", json_string(uin)),
        HostQuery::UidToUin { uid } => format!("{{uid:{}}}", json_string(uid)),
        HostQuery::MessagesById {
            chat_type,
            peer_uid,
            message_ids,
        } => format!(
            "{{chatType:{chat_type},peerUid:{},ids:{}}}",
            json_string(peer_uid),
            serde_json::to_string(message_ids).expect("message id serialization is infallible")
        ),
        HostQuery::GroupInfo { group_id } => format!("{{groupCode:{}}}", json_string(group_id)),
        HostQuery::VoiceInfo {
            peer_uid,
            message_id,
            element_id,
        } => format!(
            "{{peerUid:{},msgId:{},elementId:{},maxBytes:{MAX_VOICE_BYTES}}}",
            json_string(peer_uid),
            json_string(message_id),
            json_string(element_id),
        ),
        HostQuery::LoginInfo | HostQuery::FriendList | HostQuery::GroupList => "{}".into(),
    }
}

const COMMON_JS: &str = r#"
  function reply(status, data, code, reason) {
    var out = {status:status};
    if (data !== undefined) out.data = data;
    if (code !== undefined) out.code = code;
    if (reason !== undefined) out.reason = reason;
    return JSON.stringify(out);
  }
  function plain(v, depth, seen) {
    if (depth > 12) throw new Error('response depth over budget');
    if (v === null || typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean') return v;
    if (typeof v !== 'object' || seen.indexOf(v) >= 0) throw new Error('non-owned response');
    seen.push(v); var out;
    if (Array.isArray(v)) { if (v.length > 2048) throw new Error('response array over budget'); out = []; for (var i=0; i<v.length; i++) out.push(plain(v[i], depth+1, seen)); }
    else { var keys = Object.keys(v); if (keys.length > 256) throw new Error('response object over budget'); out = {}; keys.forEach(function(k) { out[k] = plain(v[k], depth+1, seen); }); }
    seen.pop(); return out;
  }
  function terminal(p, status, code, reason) {
    if (R.queryPending.get(p.requestId) === p) { p.status = status; p.reason = reason; R.queryPending.delete(p.requestId); if (R.queryGroupActive === p) R.queryGroupActive = null; }
    return reply(status, undefined, code, reason);
  }
  function finish(p, data) {
    try {
      var copy = plain(data, 0, []);
      if (Buffer.byteLength(JSON.stringify(copy), 'utf8') > 524288) throw new Error('response over budget');
      if (R.queryPending.get(p.requestId) === p) { p.status = 'success'; p.data = copy; }
    } catch (e) { fail(p, String(e)); }
  }
  function fail(p, reason) {
    if (R.queryPending.get(p.requestId) === p) { p.status = 'invalid_response'; p.reason = String(reason).slice(0,160); }
  }
  function settle(value, p, callback) {
    Promise.resolve(value).then(function(v) { try { callback(v); } catch (e) { fail(p, e); } }, function(e) { fail(p, e); });
  }
  function sameScope(binding, scope, sid) {
    return binding && binding.account === scope.account && binding.hostNonce === scope.hostNonce &&
      binding.sessionGeneration === scope.sessionGeneration && binding.connectionEpoch === scope.connectionEpoch &&
      binding.sid === String(sid);
  }
"#;

/// Build a fixed start script. Registration precedes every wrapper API call.
pub fn build_query_start_js(
    request_id: &str,
    scope: &QueryScope,
    query: &HostQuery,
    timeout_ms: u64,
) -> String {
    let timeout_ms = timeout_ms.min(MAX_QUERY_TIMEOUT_MS);
    let mut js = String::from(
        r#"(function() {
  var G = globalThis, R = G.__caligo_d8;
  var requestId = __REQUEST__, scope = {account:__ACCOUNT__,hostNonce:__NONCE__,sessionGeneration:__GEN__,connectionEpoch:__EPOCH__};
  var queryKind = __KIND__, args = __ARGS__, timeoutMs = __TIMEOUT__;
"#,
    );
    js.push_str(COMMON_JS);
    js.push_str(
        r#"  try {
    if (!R || typeof R !== 'object' || !R.session || !R.sid) return reply('not_ready', undefined, undefined, 'd8 session missing');
    if (!(R.queryPending instanceof Map)) {
      if (R.queryPending !== undefined) return reply('rejected', undefined, 409, 'pending store shape');
      R.queryPending = new Map();
    }
    var binding = {account:scope.account,hostNonce:scope.hostNonce,sessionGeneration:scope.sessionGeneration,connectionEpoch:scope.connectionEpoch,sid:String(R.sid)};
    if (R.queryBinding && !sameScope(R.queryBinding, scope, R.sid)) {
      var sameIdentity = R.queryBinding.account === scope.account && R.queryBinding.hostNonce === scope.hostNonce && R.queryBinding.sessionGeneration === scope.sessionGeneration;
      if (!sameIdentity) return reply('rejected', undefined, 409, 'scope binding mismatch');
      R.queryPending.forEach(function(old, oldId) { if (old.sid !== String(R.sid) || old.scope.connectionEpoch !== scope.connectionEpoch) { old.status = 'disconnected'; R.queryPending.delete(oldId); if (R.queryGroupActive === old) R.queryGroupActive = null; } });
      R.queryBinding = binding;
    }
    if (!R.queryBinding) R.queryBinding = binding;
    if (R.queryPending.size >= 32) return reply('not_ready', undefined, undefined, 'pending capacity');
    if (R.queryPending.has(requestId)) return reply('rejected', undefined, 409, 'duplicate request');
    if (typeof requestId !== 'string' || requestId.length === 0 || requestId.length > 128 || timeoutMs < 1) return reply('rejected', undefined, 400, 'invalid request');
    var p = {requestId:requestId,kind:queryKind,scope:scope,sid:String(R.sid),status:'pending',deadline:Date.now()+timeoutMs};
    R.queryPending.set(requestId, p);

    if (queryKind === 'login_info') { return terminal(p, 'not_ready', undefined, 'current account source not verified'); }
    if (queryKind === 'uin_to_uid' || queryKind === 'uid_to_uin') {
      var profile = R.session.getProfileService && R.session.getProfileService();
      var method = queryKind === 'uin_to_uid' ? profile && profile.getUidByUin : profile && profile.getUinByUid;
      if (!profile || typeof method !== 'function') { return terminal(p, 'unsupported'); }
      var key = queryKind === 'uin_to_uid' ? args.uin : args.uid;
      var result = method.call(profile, 'FriendsServiceImpl', [key]);
      settle(result, p, function(map) {
        if (!(map instanceof Map)) throw new Error('mapping result is not Map');
        var value = Map.prototype.get.call(map, key);
        if (typeof value !== 'string') throw new Error('mapping key missing');
        if (queryKind === 'uin_to_uid' && !/^u_[A-Za-z0-9_-]{1,128}$/.test(value)) throw new Error('invalid uid');
        if (queryKind === 'uid_to_uin' && !/^[1-9][0-9]{0,19}$/.test(value)) throw new Error('invalid uin');
        finish(p, queryKind === 'uin_to_uid' ? {kind:'uid',data:value} : {kind:'uin',data:value});
      });
      return reply('pending');
    }
    if (queryKind === 'messages_by_id') {
      var msg = R.session.getMsgService && R.session.getMsgService();
      if (!msg || typeof msg.getMsgsByMsgId !== 'function') { return terminal(p, 'unsupported'); }
      var peer = {chatType:args.chatType,peerUid:args.peerUid};
      settle(msg.getMsgsByMsgId.call(msg, peer, args.ids), p, function(value) {
        if (!value || (value.result !== undefined && value.result !== 0) || (value.errCode !== undefined && value.errCode !== 0) || !Array.isArray(value.msgList)) throw new Error('message result shape');
        var records = value.msgList.map(function(m) {
          var required = ['msgId','msgSeq','msgRandom','msgTime','guildId','chatType','msgType','subMsgType','peerUid','peerUin','senderUid','senderUin','elements'];
          if (!m || typeof m !== 'object') throw new Error('message shape');
          for (var i=0; i<required.length; i++) if (!(required[i] in m)) throw new Error('message field missing');
          var strings = ['msgId','msgSeq','msgRandom','msgTime','guildId','peerUid','peerUin','senderUid','senderUin'];
          for (var j=0; j<strings.length; j++) if (typeof m[strings[j]] !== 'string') throw new Error('message string shape');
          if (m.clientSeq !== undefined && m.clientSeq !== null && typeof m.clientSeq !== 'string') throw new Error('message client sequence shape');
          if (!Number.isInteger(m.chatType) || !Number.isInteger(m.msgType) || !Number.isInteger(m.subMsgType) || (m.sendStatus !== undefined && m.sendStatus !== null && !Number.isInteger(m.sendStatus))) throw new Error('message numeric shape');
          return {msgId:m.msgId,msgSeq:m.msgSeq,msgRandom:m.msgRandom,msgTime:m.msgTime,clientSeq:m.clientSeq === undefined ? null : m.clientSeq,guildId:m.guildId,chatType:m.chatType,msgType:m.msgType,subMsgType:m.subMsgType,peerUid:m.peerUid,peerUin:m.peerUin,senderUid:m.senderUid,senderUin:m.senderUin,sendStatus:m.sendStatus === undefined ? null : m.sendStatus,elements:plain(m.elements,0,[])};
        });
        finish(p, {kind:'messages',data:records});
      });
      return reply('pending');
    }
    if (queryKind === 'voice_info') {
      var voiceService = R.session.getMsgService && R.session.getMsgService();
      if (!voiceService || typeof voiceService.getMsgsByMsgId !== 'function') return terminal(p, 'unsupported');
      settle(voiceService.getMsgsByMsgId({chatType:1,peerUid:args.peerUid,guildId:''}, [args.msgId]), p, function(value) {
        if (!value || value.result !== 0 || !Array.isArray(value.msgList) || value.msgList.length !== 1) throw new Error('voice message readback shape');
        var message = value.msgList[0];
        if (!message || message.msgId !== args.msgId || message.chatType !== 1 || message.peerUid !== args.peerUid ||
            message.senderUid !== args.peerUid || typeof message.senderUin !== 'string' ||
            !/^[1-9][0-9]{0,19}$/.test(message.senderUin) || message.senderUin === scope.account ||
            !Array.isArray(message.elements) || message.elements.length > 128) throw new Error('voice message identity');
        var matches = message.elements.filter(function(item) { return item && item.elementId === args.elementId; });
        if (matches.length !== 1 || matches[0].elementType !== 4 || !matches[0].pttElement) throw new Error('voice element identity');
        var voice = matches[0].pttElement;
        if (typeof voice.fileSize !== 'string' || !/^[1-9][0-9]{0,7}$/.test(voice.fileSize) ||
            Number(voice.fileSize) > args.maxBytes || typeof voice.md5HexStr !== 'string' ||
            !/^[0-9a-fA-F]{32}$/.test(voice.md5HexStr)) throw new Error('voice metadata shape');
        finish(p, {kind:'voice_info',data:{peer_uid:args.peerUid,message_id:args.msgId,element_id:args.elementId,
          size:Number(voice.fileSize),md5:voice.md5HexStr.toLowerCase()}});
      });
      return reply('pending');
    }
    if (queryKind === 'friend_list') {
      var buddy = R.session.getBuddyService && R.session.getBuddyService();
      var prof2 = R.session.getProfileService && R.session.getProfileService();
      if (!buddy || !prof2 || typeof buddy.getBuddyListV2 !== 'function' || typeof prof2.getCoreAndBaseInfo !== 'function') { return terminal(p, 'unsupported'); }
      settle(buddy.getBuddyListV2.call(buddy, '0', true, 0), p, function(br) {
        if (!br || (br.result !== undefined && br.result !== 0) || (br.errCode !== undefined && br.errCode !== 0) || !Array.isArray(br.data)) throw new Error('buddy result shape');
        var uids = [];
        br.data.forEach(function(category) {
          if (!category || !Array.isArray(category.buddyUids)) throw new Error('buddy category shape');
          category.buddyUids.forEach(function(uid) { if (typeof uid !== 'string' || uids.indexOf(uid) >= 0) throw new Error('buddy uid shape'); uids.push(uid); });
        });
        settle(prof2.getCoreAndBaseInfo.call(prof2, 'nodeStore', uids), p, function(info) {
          if (!(info instanceof Map)) throw new Error('profile result is not Map');
          var out = uids.map(function(uid) { var item = Map.prototype.get.call(info, uid); if (!item || !item.coreInfo || typeof item.coreInfo.uin !== 'string' || typeof item.coreInfo.nick !== 'string' || typeof item.coreInfo.remark !== 'string') throw new Error('profile item missing'); return {user_id:item.coreInfo.uin,nickname:item.coreInfo.nick,remark:item.coreInfo.remark}; });
          finish(p, {kind:'friend_list',data:out});
        });
      });
      return reply('pending');
    }
    if (queryKind === 'group_list') {
      var group = R.session.getGroupService && R.session.getGroupService();
      if (!group || typeof group.getGroupList !== 'function' || typeof group.addKernelGroupListener !== 'function') { return terminal(p, 'unsupported'); }
      if (R.queryGroupActive) { R.queryPending.delete(p.requestId); return reply('rejected', undefined, 409, 'group list singleflight'); }
      if (!R.queryGroupListener) {
        R.queryGroupListener = {onGroupListUpdate:function(updateType, list) {
          if (updateType !== 0 && updateType !== 1) return;
          if (!Array.isArray(list)) return;
          var active = R.queryGroupActive;
          if (!active || active.status !== 'pending') return;
          try {
            var out = list.map(function(g) { if (!g || typeof g.groupCode !== 'string' || typeof g.groupName !== 'string' || !Number.isInteger(g.memberCount) || !Number.isInteger(g.maxMember)) throw new Error('group shape'); return {group_id:g.groupCode,group_name:g.groupName,member_count:g.memberCount,max_member_count:g.maxMember}; });
            finish(active, {kind:'group_list',data:out});
          } catch (e) { fail(active, e); }
        }};
        R.queryGroupListenerId = group.addKernelGroupListener(R.queryGroupListener);
        R.queryGroupListenerRetained = true;
      }
      R.queryGroupActive = p;
      settle(group.getGroupList.call(group, true), p, function(result) { if (result && result.result !== undefined && result.result !== 0) fail(p, 'group list result'); });
      return reply('pending');
    }
    if (queryKind === 'group_info') {
      var gs = R.session.getGroupService && R.session.getGroupService();
      if (!gs || typeof gs.getGroupDetailInfo !== 'function') { return terminal(p, 'unsupported'); }
      settle(gs.getGroupDetailInfo.call(gs, args.groupCode, 2), p, function(value) {
        if (!value || value.result !== 0 || value.groupCode !== args.groupCode || typeof value.groupName !== 'string' || !Number.isInteger(value.memberNum) || !Number.isInteger(value.maxMemberNum)) throw new Error('group detail shape');
        finish(p, {kind:'group_info',data:{group_id:value.groupCode,group_name:value.groupName,member_count:value.memberNum,max_member_count:value.maxMemberNum}});
      });
      return reply('pending');
    }
    return terminal(p, 'unsupported');
  } catch (e) {
    var pending = R && R.queryPending instanceof Map && R.queryPending.get(requestId);
    if (pending) {
      var reason = String(e).slice(0,160);
      pending.status = 'invalid_response'; pending.reason = reason; R.queryPending.delete(requestId); if (R.queryGroupActive === pending) R.queryGroupActive = null;
      return reply('invalid_response', undefined, undefined, reason);
    }
    return reply('rejected', undefined, 500, String(e).slice(0,160));
  }
})()"#,
    );
    for (needle, value) in [
        ("__REQUEST__", json_string(request_id)),
        ("__ACCOUNT__", json_string(&scope.account)),
        ("__NONCE__", json_string(&scope.host_nonce.to_string())),
        (
            "__GEN__",
            json_string(&scope.session_generation.to_string()),
        ),
        (
            "__EPOCH__",
            json_string(&scope.connection_epoch.to_string()),
        ),
        ("__KIND__", json_string(kind(query))),
        ("__TIMEOUT__", timeout_ms.to_string()),
        ("__ARGS__", query_args(query)),
    ] {
        js = js.replace(needle, &value);
    }
    js
}

/// Build a poll script. It performs no wrapper API call.
pub fn build_query_poll_js(request_id: &str, scope: &QueryScope) -> String {
    let mut js = String::from(
        r#"(function() {
  var G=globalThis,R=G.__caligo_d8,id=__REQUEST__,account=__ACCOUNT__,nonce=__NONCE__,gen=__GEN__,epoch=__EPOCH__;
  function out(status,data,code,reason){var v={status:status};if(data!==undefined)v.data=data;if(code!==undefined)v.code=code;if(reason!==undefined)v.reason=reason;return JSON.stringify(v);}
  if(!R||!(R.queryPending instanceof Map)||!sameScope(R.queryBinding,{account:account,hostNonce:nonce,sessionGeneration:gen,connectionEpoch:epoch},R.sid))return out('rejected',undefined,409,'scope binding mismatch');
  var p=R.queryPending.get(id);if(!p)return out('rejected',undefined,404,'request missing');
  if(p.scope.account!==account||p.scope.hostNonce!==nonce||p.scope.sessionGeneration!==gen||p.scope.connectionEpoch!==epoch||p.sid!==String(R.sid))return out('rejected',undefined,409,'request binding mismatch');
  if(p.status==='pending'&&Date.now()>=p.deadline){p.status='timeout';p.reason='query timeout';}
  if(p.status==='pending')return out('pending');
  var result=out(p.status,p.data,p.status==='invalid_response'?422:undefined,p.reason);R.queryPending.delete(id);if(R.queryGroupActive===p)R.queryGroupActive=null;return result;
})()"#,
    );
    for (needle, value) in [
        ("__REQUEST__", json_string(request_id)),
        ("__ACCOUNT__", json_string(&scope.account)),
        ("__NONCE__", json_string(&scope.host_nonce.to_string())),
        (
            "__GEN__",
            json_string(&scope.session_generation.to_string()),
        ),
        (
            "__EPOCH__",
            json_string(&scope.connection_epoch.to_string()),
        ),
    ] {
        js = js.replace(needle, &value);
    }
    let mut prefix = String::from(
        "function sameScope(b,s,sid){return b&&b.account===s.account&&b.hostNonce===s.hostNonce&&b.sessionGeneration===s.sessionGeneration&&b.connectionEpoch===s.connectionEpoch&&b.sid===String(sid);}\n",
    );
    prefix.push_str(&js);
    prefix
}

/// Build cancellation without unregistering the retained group listener.
pub fn build_query_cancel_js(request_id: &str, scope: &QueryScope) -> String {
    let mut js = String::from(
        r#"(function(){var R=globalThis.__caligo_d8,id=__REQUEST__,s={account:__ACCOUNT__,hostNonce:__NONCE__,sessionGeneration:__GEN__,connectionEpoch:__EPOCH__};function same(b){return b&&b.account===s.account&&b.hostNonce===s.hostNonce&&b.sessionGeneration===s.sessionGeneration&&b.connectionEpoch===s.connectionEpoch&&b.sid===String(R.sid);}if(!R||!(R.queryPending instanceof Map)||!same(R.queryBinding))return JSON.stringify({status:'rejected',code:409,reason:'scope binding mismatch'});var p=R.queryPending.get(id);if(!p)return JSON.stringify({status:'rejected',code:404,reason:'request missing'});p.status='cancelled';p.reason='cancelled by caller';R.queryPending.delete(id);if(R.queryGroupActive===p)R.queryGroupActive=null;return JSON.stringify({status:'cancelled'});})()"#,
    );
    for (needle, value) in [
        ("__REQUEST__", json_string(request_id)),
        ("__ACCOUNT__", json_string(&scope.account)),
        ("__NONCE__", json_string(&scope.host_nonce.to_string())),
        (
            "__GEN__",
            json_string(&scope.session_generation.to_string()),
        ),
        (
            "__EPOCH__",
            json_string(&scope.connection_epoch.to_string()),
        ),
    ] {
        js = js.replace(needle, &value);
    }
    js
}

/// Decode a terminal result. A pending response is returned as an error so
/// callers that need a terminal value cannot accidentally consume it.
pub fn decode_query_result(
    query: &HostQuery,
    scope: &QueryScope,
    json: &str,
) -> Result<QueryOutcome, &'static str> {
    decode_query_poll(query, scope, json)?.ok_or("query still pending")
}

/// Decode one poll. `Ok(None)` means the request remains pending.
pub fn decode_query_poll(
    query: &HostQuery,
    scope: &QueryScope,
    json: &str,
) -> Result<Option<QueryOutcome>, &'static str> {
    if json.len() > MAX_QUERY_BYTES {
        return Err("query response over budget");
    }
    let value: Value = serde_json::from_str(json).map_err(|_| "query response json")?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .ok_or("query response status")?;
    let outcome = match status {
        "pending" => return Ok(None),
        "success" => {
            let data: QueryData =
                serde_json::from_value(value.get("data").cloned().ok_or("query response data")?)
                    .map_err(|_| "query response data shape")?;
            data.validate_for(query, scope)?;
            QueryOutcome::Success(data)
        }
        "unsupported" => QueryOutcome::Unsupported,
        "not_ready" => QueryOutcome::NotReady,
        "timeout" => QueryOutcome::Timeout,
        "cancelled" => QueryOutcome::Disconnected,
        "invalid_response" => QueryOutcome::InvalidResponse,
        "rejected" => QueryOutcome::Rejected {
            code: value.get("code").and_then(Value::as_i64).unwrap_or(400),
            reason: value
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("rejected")
                .to_owned(),
        },
        _ => return Err("unknown query response status"),
    };
    Ok(Some(outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_is_not_advertised_and_shapes_are_strict() {
        assert!(!is_supported(&HostQuery::LoginInfo));
        assert!(is_supported(&HostQuery::FriendList));
        let scope = QueryScope {
            account: "10001".into(),
            host_nonce: 1,
            session_generation: 2,
            connection_epoch: 3,
        };
        let q = HostQuery::UinToUid { uin: "9".into() };
        assert!(decode_query_result(
            &q,
            &scope,
            r#"{"status":"success","data":{"kind":"uin","data":"9"}}"#
        )
        .is_err());
    }

    #[cfg(feature = "research")]
    #[test]
    fn voice_profile_rejects_missing_duplicate_and_wrong_modules() {
        assert!(!matches_voice_profile(&[]));
        let duplicate = vec![
            "C:/fixture/one/wrapper.node".into(),
            "C:/fixture/two/WRAPPER.NODE".into(),
        ];
        assert!(!matches_voice_profile(&duplicate));
        let dir = std::env::temp_dir().join(format!("caligo-voice-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wrapper = dir.join("wrapper.node");
        std::fs::write(&wrapper, b"synthetic module").unwrap();
        assert!(!matches_voice_profile(&[wrapper.to_str().unwrap().to_owned()]));
        std::fs::remove_file(&wrapper).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
