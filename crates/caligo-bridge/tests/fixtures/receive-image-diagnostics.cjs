const assert = require('node:assert/strict');
const vm = require('node:vm');
const scripts = JSON.parse(require('node:fs').readFileSync(0, 'utf8'));
let listener;
const service = {sendMsg() {}, addKernelMsgListener(v) {listener = v; return '1';}};
const session = {getSessionId: () => '1', getMsgService: () => service};
const sandbox = vm.createContext({Buffer, process: {_linkedBinding: () => ({
  NodeIQQNTWrapperSession: {getNTWrapperSession: () => session}
})}});
assert.match(vm.runInContext(scripts.pilot, sandbox), /^ARMED/);
function row(picType = 1001) {
  return {msgId:'5001',msgSeq:'2',msgRandom:'3',msgTime:'1794567890',guildId:'SECRET-cookie',
    peerUid:'SECRET-peer',peerUin:'20002',senderUid:'SECRET-peer',senderUin:'20002',
    chatType:1,msgType:2,subMsgType:0,elements:[{elementType:2,elementId:'9',
      picElement:{picType,picSubType:0,fileSize:'70',picWidth:1,picHeight:1,
        md5HexStr:'e605ef58c3bb5f33463782ab90d58295',filePath:'SECRET-path',fileUuid:'SECRET-token'}}]};
}
// The existing PNG-only inbound gate remains unchanged; the default type is observed but rejected.
listener.onRecvMsg([row(), row(1000)]);
let batch = JSON.parse(vm.runInContext(scripts.poll, sandbox));
assert.equal(batch.items.length, 1);
assert.equal(batch.rejected, 1);
assert.equal(batch.imageShapes.length, 2);
assert.equal(batch.imageShapes[0].pic_type, 'png');
assert.equal(batch.imageShapes[1].pic_type, 'default');
assert.equal(batch.imageShapes[1].peer_matches, true);
assert.equal(batch.imageShapes[1].size_valid, true);
assert.ok(!JSON.stringify(batch.imageShapes).includes('SECRET'));
assert.deepEqual(JSON.parse(vm.runInContext(scripts.poll, sandbox)).imageShapes, []);
// Rejection before element parsing, mixed elements and private identity mismatch are distinguishable.
const badType = row(); badType.msgSeq = 2;
const mixed = row(); mixed.elements.push({elementType:1,textElement:{content:'SECRET-body'}});
const wrongPeer = row(); wrongPeer.senderUid = 'SECRET-other';
listener.onRecvMsg([badType, mixed, wrongPeer]);
batch = JSON.parse(vm.runInContext(scripts.poll, sandbox));
assert.equal(batch.items.length, 0);
assert.equal(batch.rejected, 3);
assert.equal(batch.imageShapes[0].wire_strings_valid, false);
assert.equal(batch.imageShapes[1].arity, 'multiple');
assert.equal(batch.imageShapes[2].peer_matches, false);
assert.ok(!JSON.stringify(batch.imageShapes).includes('SECRET'));
// The diagnostic budget cannot change record admission or stop draining once exhausted.
listener.onRecvMsg(Array.from({length:20}, () => row()));
batch = JSON.parse(vm.runInContext(scripts.poll, sandbox));
assert.equal(batch.items.length, 20);
assert.equal(batch.rejected, 0);
assert.equal(batch.imageShapes.length, 8);
// Diagnostic getter errors also leave the existing admission contract in control.
const getter = row();
Object.defineProperty(getter.elements[0].picElement, 'picType', {get() {throw Error('SECRET-error');}});
listener.onRecvMsg([getter]);
batch = JSON.parse(vm.runInContext(scripts.poll, sandbox));
assert.equal(batch.items.length, 0);
assert.equal(batch.rejected, 1);
assert.ok(!JSON.stringify(batch).includes('SECRET-error'));
