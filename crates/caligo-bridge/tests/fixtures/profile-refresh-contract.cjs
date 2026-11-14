const assert = require('node:assert/strict');
const vm = require('node:vm');
const scripts = globalThis.caligoRefreshScripts;
const tick = () => new Promise(resolve => setImmediate(resolve));
let cases = 0;
function fixture(options = {}) {
  const uin = '1234567890', uid = 'u_synthetic';
  let listener, calls = 0, removed = 0, now = 1000, sid = 'nt_1', resolveAck, rejectAck;
  const row = (nickname = 'Synthetic') => ({uid, uin, simpleInfo: {coreInfo: {uid, uin, nick: nickname}}, secret: 'never copy'});
  const ack = () => ({result: 0, source: 1, errMsg: '', detail: new Map([[uid, row()]])});
  const profile = {
    addKernelProfileListener(value) {listener = value; return '9007199254740993';},
    removeKernelProfileListener(id) {
      assert.equal(id, '9007199254740993'); removed++;
      if (options.removeThrows) throw Error('synthetic remove failure');
    },
    getSelfStatus() {listener.onSelfStatusChanged({uid, uin, status: 10}); return {result: 0};},
    getCoreAndBaseInfo() {return new Map([[uid, {coreInfo: {uid, uin, nick: 'Synthetic'}}]]);},
    fetchUserDetailInfo(trace, ids, source, biz) {
      calls++;
      assert.equal(arguments.length, 4);
      assert.equal(trace, 'BuddyProfileStore');
      assert.deepEqual([...ids], [uid]); assert.equal(source, 1); assert.deepEqual([...biz], [0]);
      if (options.throwFetch) throw Error('synthetic fetch throw');
      if (options.unrelated) listener.onUserDetailInfoChanged({...row(), uid: 'u_unrelated'});
      if (!options.noEvent) listener.onUserDetailInfoChanged(options.event ? options.event(row()) : row());
      if (options.duplicate) listener.onUserDetailInfoChanged(row());
      if (options.conflict) listener.onUserDetailInfoChanged(row('Changed'));
      if (options.deferred) return new Promise((resolve, reject) => {resolveAck = resolve; rejectAck = reject;});
      if (options.reject) return Promise.reject(Error('synthetic rejection'));
      return options.ack ? options.ack(ack(), row()) : ack();
    }
  };
  const S = {
    phase: 'profile_ready', profile, session: {getSessionId: () => sid},
    forwardMethod: () => new Map([[uin, uid]]), reverseMethod: () => new Map([[uid, uin]])
  };
  const sandbox = vm.createContext({Buffer, Date: {now: () => now}, __caligo_uid_map_main: S});
  const run = name => vm.runInContext(scripts[name], sandbox);
  return {S, sandbox, run, row, ack,
    counts: () => ({calls, removed}),
    advance: () => {now += 5001;}, changeSession: () => {sid = 'nt_2';},
    event: value => listener.onUserDetailInfoChanged(value),
    offline: () => listener.onSelfStatusChanged({uid, uin, status: 20}),
    resolve: () => resolveAck(ack()), reject: () => rejectAck(Error('late rejection')),
    async begin() {
      assert.equal(run('start'), 'DISPATCHED'); await tick();
      assert.equal(run('identity'), 'POLL'); await tick();
      assert.equal(run('identity'), 'POLL');
      assert.equal(S.status, 'current_login_match');
      assert.equal(run('refresh'), 'POLL'); await tick();
    },
    finish() {run('refresh'); return JSON.parse(run('poll'));}
  };
}
(async () => {
  let f = fixture({unrelated: true, duplicate: true}); await f.begin();
  let out = f.finish();
  assert.equal(out.status, 'profile_refresh_match');
  assert.equal(out.listenerRegistration, 'remove_returned');
  assert.equal(out.profileRefresh.profile.nickname, 'Synthetic');
  assert.equal(out.profileRefresh.notification_request_causality_proven, false);
  assert.equal(JSON.stringify(out).includes('never copy'), false);
  assert.deepEqual(f.counts(), {calls: 1, removed: 1});
  assert.equal(f.sandbox.__caligo_uid_map_main, undefined); cases++;

  const failures = [
    [{ack: a => ({...a, source: 0})}, 'profile_refresh_ack_invalid'],
    [{ack: a => ({...a, result: 1})}, 'profile_refresh_ack_invalid'],
    [{ack: a => ({...a, errMsg: null})}, 'profile_refresh_ack_invalid'],
    [{ack: a => ({...a, detail: {}})}, 'profile_refresh_detail_invalid'],
    [{ack: a => ({...a, detail: new Map()})}, 'profile_refresh_detail_invalid'],
    [{ack: (a, r) => ({...a, detail: new Map([[r.uid, {...r, uin: '1234567891'}]])})}, 'profile_refresh_detail_invalid'],
    [{event: r => ({...r, uin: '1234567891'})}, 'profile_refresh_notification_invalid'],
    [{event: r => ({...r, simpleInfo: null})}, 'profile_refresh_notification_invalid'],
    [{event: r => ({...r, simpleInfo: {coreInfo: {...r.simpleInfo.coreInfo, nick: 'x'.repeat(4097)}}})}, 'profile_refresh_notification_invalid'],
    [{event: r => ({...r, simpleInfo: {coreInfo: {...r.simpleInfo.coreInfo, nick: 'Different'}}})}, 'profile_refresh_reply_notification_mismatch'],
    [{conflict: true}, 'profile_refresh_notification_conflict'],
    [{reject: true}, 'profile_refresh_rejected'],
    [{throwFetch: true}, 'profile_refresh_threw']
  ];
  for (const [options, status] of failures) {
    f = fixture(options); await f.begin(); out = f.finish();
    assert.equal(out.status, status); assert.equal(out.profileRefresh, undefined);
    assert.equal(out.listenerRegistration, 'remove_returned');
    assert.deepEqual(f.counts(), {calls: 1, removed: 1}); cases++;
  }
  // Per-request reply is required even when a matching unsolicited event exists.
  f = fixture({deferred: true}); await f.begin();
  assert.equal(f.run('poll'), 'PENDING'); f.advance(); out = f.finish();
  assert.equal(out.status, 'profile_refresh_timeout');
  f.reject(); f.event(f.row()); await tick();
  assert.equal(f.sandbox.__caligo_uid_map_main, undefined); cases++;

  // Reply alone cannot stand in for the notification; ack-first is supported.
  f = fixture({noEvent: true, unrelated: true}); await f.begin();
  f.run('refresh'); assert.equal(f.run('poll'), 'PENDING');
  f.event(f.row()); out = f.finish(); assert.equal(out.status, 'profile_refresh_match'); cases++;
  f = fixture({noEvent: true}); await f.begin(); f.advance();
  assert.equal(f.finish().status, 'profile_refresh_timeout'); cases++;

  f = fixture({deferred: true}); await f.begin(); f.changeSession();
  assert.equal(f.finish().status, 'profile_refresh_session_changed'); f.resolve(); await tick(); cases++;
  f = fixture({deferred: true}); await f.begin(); f.offline();
  assert.equal(f.finish().status, 'self_status_offline'); f.reject(); await tick(); cases++;

  f = fixture({deferred: true}); await f.begin();
  assert.equal(f.run('cancel'), 'CANCELLED'); f.resolve(); f.event(f.row()); await tick();
  assert.equal(f.sandbox.__caligo_uid_map_main, undefined); cases++;
  f = fixture({removeThrows: true}); await f.begin(); out = f.finish();
  assert.equal(out.status, 'self_status_listener_cleanup_unverified');
  assert.equal(out.profileRefresh, undefined); assert.equal(out.currentLogin, undefined);
  assert.equal(f.run('state'), 'BUSY'); assert.equal(f.run('cancel'), 'CLEANUP_UNKNOWN');
  assert.equal(f.run('state'), 'BUSY'); assert.equal(f.counts().removed, 1); cases++;
  f = fixture({deferred: true, removeThrows: true}); await f.begin();
  assert.equal(f.run('cancel'), 'CLEANUP_UNKNOWN'); f.reject(); await tick();
  assert.equal(f.run('state'), 'BUSY'); assert.equal(f.counts().removed, 1); cases++;
  console.log(`profile refresh contract: ${cases} scenarios PASS`);
})().catch(error => {console.error(error); process.exitCode = 1;});
