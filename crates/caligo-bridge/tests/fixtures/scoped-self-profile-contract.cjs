const assert = require('node:assert/strict');
const vm = require('node:vm');
const scripts = globalThis.caligoScopedProfileScripts;
const tick = () => new Promise(resolve => setImmediate(resolve));
function fixture(options = {}) {
  const uid = 'u_synthetic';
  let account = options.wrongAccount ? '10002' : '10001';
  let listener, fetches = 0, removed = 0, pending = [], removeThrows = false;
  const row = nick => ({uid, uin:account, simpleInfo:{coreInfo:{uid,uin:account,nick}}});
  const ack = nick => ({result:0, source:1, errMsg:'', detail:new Map([[uid,row(nick)]])});
  const profile = {
    addKernelProfileListener(value) {listener=value; return '1';},
    removeKernelProfileListener() {removed++; if (removeThrows) throw Error('synthetic');},
    getSelfStatus() {listener.onSelfStatusChanged({uid,uin:account,status:10}); return {result:0};},
    getUidByUin() {return new Map([[account,uid]]);},
    getUinByUid() {return new Map([[uid,account]]);},
    getCoreAndBaseInfo() {return new Map([[uid,{coreInfo:{uid,uin:account,nick:'Cached'}}]]);},
    fetchUserDetailInfo(trace, ids, source, biz) {
      assert.equal(trace,'BuddyProfileStore');assert.deepEqual([...ids],[uid]);
      assert.equal(source,1);assert.deepEqual([...biz],[0]);fetches++;
      const ownListener=listener;
      return new Promise((resolve,reject)=>pending.push({resolve,reject,notify:nick=>ownListener.onUserDetailInfoChanged(row(nick))}));
    }
  };
  if (options.missingListener) delete profile.addKernelProfileListener;
  const sandbox=vm.createContext({Buffer,require(path) {
    assert.equal(path,'synthetic-wrapper.node');
    return {NodeIQQNTWrapperSession:{getNTWrapperSession(key) {
      assert.equal(key,'nt_1');return {getSessionId:()=>key,getProfileService:()=>profile};
    }}};
  }});
  const run=name=>vm.runInContext(scripts[name],sandbox);
  return {run,sandbox,pending,counts:()=>({fetches,removed}),throwOnRemove:()=>{removeThrows=true;},
    async begin() {assert.equal(run('start'),'DISPATCHED');await tick();assert.equal(run('poll'),'PENDING');await tick();},
    async dispatch() {await this.begin();assert.equal(run('poll'),'PENDING');assert.equal(fetches,pending.length);}
  };
}
(async()=>{
  let f=fixture({wrongAccount:true});assert.equal(f.run('start'),'DISPATCHED');await tick();
  assert.equal(JSON.parse(f.run('poll')).status,'current_account_mismatch');assert.equal(f.counts().fetches,0);
  f=fixture({missingListener:true});assert.equal(f.run('start'),'SURFACE_DONE');
  assert.equal(JSON.parse(f.run('poll')).status,'self_status_listener_surface_missing');
  assert.equal(f.sandbox.__caligo_uid_map_main,undefined);
  f=fixture();await f.dispatch();f.pending[0].notify('Fresh');f.pending[0].resolve({result:0,source:1,errMsg:'',detail:new Map([['u_synthetic',{uid:'u_synthetic',uin:'10001',simpleInfo:{coreInfo:{uid:'u_synthetic',uin:'10001',nick:'Fresh'}}}]])});
  await tick();let out=JSON.parse(f.run('poll'));
  assert.equal(out.status,'profile_refresh_match');assert.equal(out.currentLogin.nickname,'Cached');
  assert.equal(out.profileRefresh.profile.nickname,'Fresh');assert.equal(out.listenerRegistration,'remove_returned');
  assert.equal(f.sandbox.__caligo_uid_map_main,undefined);
  // A revoked request can settle during a new request without changing the new state.
  f=fixture();await f.dispatch();const old=f.pending[0];assert.equal(f.run('cancel'),'CANCELLED');
  await f.dispatch();old.notify('Stale');old.reject(Error('late old request'));await tick();
  assert.equal(f.run('poll'),'PENDING');assert.equal(f.sandbox.__caligo_uid_map_main.profileRefreshNotification,undefined);
  assert.equal(f.run('cancel'),'CANCELLED');f.pending[1].reject(Error('late new request'));await tick();
  assert.deepEqual(f.counts(),{fetches:2,removed:2});
  // Unverified cancellation blocks another dispatch and never retries removal.
  f=fixture();await f.dispatch();f.throwOnRemove();assert.equal(f.run('cancel'),'CLEANUP_UNKNOWN');
  f.pending[0].reject(Error('late'));await tick();assert.equal(f.run('start'),'BUSY');
  assert.deepEqual(f.counts(),{fetches:1,removed:1});
  console.log('scoped self profile contract: 5 scenarios PASS');
})().catch(error=>{console.error(error);process.exitCode=1;});
