(function () {
  var S = globalThis.__caligo_uid_map_main;
  if (!S) return 'ERR:no profile-refresh state';
  function fail(status) { S.status = status; S.phase = 'done'; delete S.profileRefresh; }
  function current() {
    if (globalThis.__caligo_uid_map_main !== S || S.phase !== 'profile_refresh_pending') return false;
    try {
      if (S.session.getSessionId() !== S.sessionId) { fail('profile_refresh_session_changed'); return false; }
    } catch (_) { fail('profile_refresh_session_unknown'); return false; }
    return true;
  }
  function row(value) {
    var identity = S.currentLogin;
    if (!value || value.uid !== identity.uid || value.uin !== identity.uin) throw new Error('identity mismatch');
    var core = value.simpleInfo && value.simpleInfo.coreInfo;
    if (!core || core.uid !== identity.uid || core.uin !== identity.uin ||
        typeof core.nick !== 'string' || !core.nick.trim() || Buffer.byteLength(core.nick, 'utf8') > 4096) {
      throw new Error('core profile mismatch');
    }
    // Only copy the bounded fields needed for this contract. Never serialize
    // the native object, account credentials or the rest of the profile.
    return {uid: identity.uid, uin: identity.uin, nickname: core.nick};
  }
  if (!S.profileRefreshStarted) {
    if (S.phase !== 'done' || S.status !== 'current_login_match') return 'POLL';
    if (S.nicknameStatus !== 'matched' || !S.currentLogin || S.listenerRegistration !== 'registered') {
      fail('profile_refresh_identity_unverified'); return 'POLL';
    }
    if (typeof S.profile.fetchUserDetailInfo !== 'function') {
      fail('profile_refresh_accessor_missing'); return 'POLL';
    }
    S.profileRefreshStarted = true;
    S.retainOnCleanupUnknown = true;
    S.route = 'wrapper.node nt_1 ProfileService.fetchUserDetailInfo KSERVER';
    S.phase = 'profile_refresh_pending';
    S.selfStatus = {uid: S.currentLogin.uid, uin: S.currentLogin.uin, status: S.currentLogin.status};
    S.profileRefreshDeadline = Date.now() + 5000;
    S.profileRefreshEvent = function (value) {
      if (!current()) return;
      // Unrelated account notifications are normal fanout, not this request.
      if (!value || value.uid !== S.currentLogin.uid) return;
      try {
        var copy = row(value);
        if (S.profileRefreshNotification && JSON.stringify(S.profileRefreshNotification) !== JSON.stringify(copy)) {
          fail('profile_refresh_notification_conflict'); return;
        }
        S.profileRefreshNotification = copy;
      } catch (_) { fail('profile_refresh_notification_invalid'); }
    };
    if (!current()) return 'POLL';
    S.profileRefreshCallAttempted = true;
    try {
      var result = S.profile.fetchUserDetailInfo('BuddyProfileStore', [S.currentLogin.uid], 1, [0]);
      Promise.resolve(result).then(function (ack) {
        if (!current()) return;
        try {
          if (!ack || ack.result !== 0 || ack.source !== 1 ||
              typeof ack.errMsg !== 'string' || Buffer.byteLength(ack.errMsg, 'utf8') > 4096) {
            fail('profile_refresh_ack_invalid'); return;
          }
          // The native callback owns this Promise. Map#get rejects a spoofed
          // object and supports a Map created in another V8 context.
          var detail = Map.prototype.get.call(ack.detail, S.currentLogin.uid);
          S.profileRefreshReply = row(detail);
        } catch (_) { fail('profile_refresh_detail_invalid'); }
      }, function () { if (current()) fail('profile_refresh_rejected'); });
    } catch (_) { if (current()) fail('profile_refresh_threw'); }
    return 'POLL';
  }
  if (S.phase !== 'profile_refresh_pending' || !current()) return 'POLL';
  if (Date.now() >= S.profileRefreshDeadline) { fail('profile_refresh_timeout'); return 'POLL'; }
  if (!S.profileRefreshReply || !S.profileRefreshNotification) return 'POLL';
  if (JSON.stringify(S.profileRefreshReply) !== JSON.stringify(S.profileRefreshNotification)) {
    fail('profile_refresh_reply_notification_mismatch'); return 'POLL';
  }
  S.profileRefresh = {
    requested_source: 1, returned_source: 1,
    profile: S.profileRefreshReply,
    matching_notification_observed: true,
    notification_request_causality_proven: false
  };
  S.status = 'profile_refresh_match';
  S.phase = 'done';
  return 'POLL';
})()
