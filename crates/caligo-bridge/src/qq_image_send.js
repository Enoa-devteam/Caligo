function (args) {
  const reply = value => JSON.stringify(value);
  const failure = error => reply({err:error});
  const R = globalThis.__caligo_res;
  if (!R || R.stopped || !/^[1-9][0-9]{0,19}$/.test(args.account) || R.voiceAccount !== args.account)
    return failure('image_profile_unavailable');
  const chatType = args.chatType;
  if ((chatType !== 1 && chatType !== 2) ||
      !/^[1-9][0-9]{0,19}$/.test(args.peerUin) || (chatType === 1 && args.peerUin === args.account) ||
      !/^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$/.test(args.requestId) ||
      !Number.isInteger(args.wireSeq) || args.wireSeq <= 0 ||
      !Number.isInteger(args.timeoutMs) || args.timeoutMs <= 0)
    return failure('invalid_image_request');
  if (Object.hasOwn(R.sends, args.requestId) || Object.hasOwn(R.sendResults, args.requestId) ||
      Object.keys(R.sends).length >= 128)
    return failure('send_capacity_or_duplicate');

  const deadline = Date.now() + args.timeoutMs;
  let cookie, peerUid, element, image;
  try {
    const fs = process.getBuiltinModule('node:fs');
    const path = process.getBuiltinModule('node:path');
    const crypto = process.getBuiltinModule('node:crypto');
    peerUid = args.peerUin;
    if (chatType === 1) {
      const profile = R.session.getProfileService();
      const mapped = profile.getUidByUin('FriendsServiceImpl', [args.peerUin]);
      peerUid = mapped instanceof Map && Map.prototype.get.call(mapped, args.peerUin);
      if (typeof peerUid !== 'string' || !/^u_[A-Za-z0-9_-]{1,128}$/.test(peerUid))
        return failure('private_uid_mapping_unavailable');
    }
    const serverTime = R.session.getMSFService().getServerTime();
    if (typeof serverTime !== 'string' || !serverTime || serverTime === '0')
      return failure('server_time_unavailable');
    cookie = R.ms.generateMsgUniqueId(chatType, serverTime);
    if (typeof cookie !== 'string' || !cookie || cookie === '0' ||
        Object.values(R.sends).some(send => send.cookie === cookie))
      return failure('invalid_cookie');

    const bytes = Buffer.from(args.png, 'hex');
    const md5 = crypto.createHash('md5').update(bytes).digest('hex');
    const sha256 = crypto.createHash('sha256').update(bytes).digest('hex');
    image = {size:bytes.length, md5, width:args.width, height:args.height, picType:1000};
    const name = md5 + '.png';
    const dest = R.ms.getRichMediaFilePathForGuild({
      md5HexStr:md5, fileName:name, elementType:2, elementSubType:0,
      thumbSize:0, needCreate:true, downloadType:1, file_uuid:''
    });
    const windows = process.platform === 'win32';
    const samePath = (left, right) => windows ? left.toLowerCase() === right.toLowerCase() : left === right;
    if (typeof dest !== 'string' || !path.isAbsolute(dest) || dest !== path.normalize(dest) ||
        (windows && !/^[a-zA-Z]:[\\/]/.test(dest)) || !samePath(path.basename(dest), name))
      return failure('invalid_image_storage_path');
    const parent = path.dirname(dest);
    fs.mkdirSync(parent, {recursive:true});
    if (!samePath(fs.realpathSync(parent), parent)) return failure('invalid_image_storage_path');
    // Create exclusively. A preexisting cache entry can only be reused when
    // its bytes match, and files handed to QQ stay owned by its cache.
    try {
      fs.writeFileSync(dest, bytes, {flag:'wx'});
    } catch (error) {
      if (error.code !== 'EEXIST') throw error;
    }
    const entry = fs.lstatSync(dest);
    if (!entry.isFile() || entry.size !== image.size || !samePath(fs.realpathSync(dest), dest))
      return failure('image_storage_mismatch');
    const fd = fs.openSync(dest, 'r');
    try {
      const opened = fs.fstatSync(fd);
      if (!opened.isFile() || opened.dev !== entry.dev || opened.ino !== entry.ino ||
          opened.size !== image.size) return failure('image_storage_mismatch');
      const stored = Buffer.alloc(image.size);
      let offset = 0;
      while (offset < stored.length) {
        const read = fs.readSync(fd, stored, offset, stored.length - offset, offset);
        if (!read) return failure('image_storage_mismatch');
        offset += read;
      }
      if (fs.fstatSync(fd).size !== image.size ||
          crypto.createHash('sha256').update(stored).digest('hex') !== sha256)
        return failure('image_storage_mismatch');
    } finally {
      fs.closeSync(fd);
    }
    element = {elementType:2, elementId:'', picElement:{
      md5HexStr:md5, fileSize:String(image.size), picWidth:image.width, picHeight:image.height,
      fileName:name, sourcePath:dest, original:true, picType:image.picType, picSubType:0,
      fileUuid:'', fileSubId:'', thumbFileSize:0, summary:''
    }};
  } catch (_) {
    return failure('image_preparation_failed');
  }
  if (R.stopped || Date.now() >= deadline) return failure('image_preparation_expired');

  const key = args.requestId;
  const pending = {cookie, chat:chatType, peer:peerUid, peerUin:args.peerUin, image, selfUin:args.account,
    wireSeq:args.wireSeq, deadline, addId:null, updateId:null, done:false};
  R.sends[key] = pending;
  R.sendResults[key] = {status:'pending'};
  function result(status, detail) {
    if (R.stopped || R.sends[key] !== pending) return;
    pending.promiseDone = true;
    pending.promiseStatus = status;
    if (pending.readback) {
      R.settleSendResult(R, key, pending);
    } else if (!pending.ambiguous && !pending.timedOut) {
      R.sendResults[key] = detail;
    }
    if (status !== 'accepted') pending.failed = true;
  }
  const unknown = () => result('unknown', {status:'unknown', reason:'image_submit_unknown'});
  try {
    const promise = R.ms.sendMsg('0', {chatType, peerUid, guildId:cookie}, [element], new Map());
    Promise.resolve(promise).then(value => {
      if (value && value.result === 0) {
        result('accepted', {status:'accepted'});
      } else if (value && Number.isInteger(value.result) && !pending.addId) {
        result('rejected', {status:'rejected', error:'native image rejected'});
      } else {
        unknown();
      }
    }, unknown);
  } catch (_) {
    unknown();
  }
  return reply({fired:true, mid:key});
}
