# 响应侧拒绝码消费者 — 范围与锚点(下一工作块起点)

日期:2026-10-09。原始范围与静态合同记录见下文;当前实现状态以本节开头的
2026-10-16 更新为准。
动机:群聊 @ 消息 r20–r28b 八轮静默拒(普通文本同桥 PASS,见
`g3-send-27624-r2x.jsonl`),无 retCode 无法继续定位;发送失败可观测性
是 @ 元素与一切后续功能(回复/转发)的调试前提。

## 2026-10-16 当前状态

- `consumer_probe::consumer_invoke` 与固定-hash `73BD6C` 的五参 ABI 对齐。现在回调期间将
  第五参按有界 libc++ string 形态复制到预分配槽;resident worker 在同一条记录中写入状态、长度,
  本地 request cookie 与 response UID 的 domain-separated keyed fingerprint;两者原值均不落盘。
  SSO 长度超过 22、不可读或超预算均失败关闭;enqueue 后立即擦除回调栈副本(含队列溢出),
  worker 消费后擦除其副本和槽。
 实现/合成测试只证明捕获合同,不证明 QQ 现场 callback 已触发。
- fingerprint 只供同一进程内后续对比,绝不是 native ID,也没有进入账本或成功回执。
  `code==0` 仍是 `BusinessAccepted`;回调 cookie、response UID 与 CPB 身份之间尚无已证桥。
- `G3NativeOps::native_submit` 目前只将本地 cookie 放入 callback delegate;群聊 protobuf
  由 `encode_group_text` 编码,没有从该 cookie 到 QQ `msgUid` 的已证请求字段。CPB decoder
  当前只填已证 seq/time/peer/sender UID/session/text,`native_id` 与 `sender_uin` 继续为空。
  R3 强身份与 UID→UIN 仍未闭合,不得用正文/时间/seq 相似度替代。
- 对本机 `_reference/caligo-n3-resident-20261009/` 的历史 R10 样本仅核对字段结构:
  `r10-sends.json` 与 `g2x-r10-capture.jsonl` 没有 response callback UID/cookie 字段。
  该接收侧样本不能证明 request cookie→response UID 或 response UID→CPB native ID。
  原文提到的 repository ledger `2026-10-10` 小节在当前仓库中不存在,该引用已收窄为本地样本及此边界。
- 2026-10-16 用户授权的一次 UIN-only G3 请求未在目标会话出现;独立 90 秒 resident drain
  没有 callback result/worker/orphan diagnostic。随后源码检查发现 direct G3 的旧 delegate
  把 `ConsumerCtx*` 当作 cookie,而 `consumer_invoke` 会将它识别为上下文指针并拒绝作为
  per-send cookie。已改为每请求独立的 `{generation, unique nonce}` 并固定配对报告路径,定向测试 11/11、`init.ps1 -Mode all`
  默认与 research 均通过。这解释了该探针可能无法产生日志,但不证明 QQ 曾调用 callback,
  也不解释消息未出现;两项现场结果都保持 unknown。用户正常重启后 PID 40240 已退出;新根进程 PID 34760 未加载 Caligo。
  修正后的 research DLL 已离线构建,hash 为 `F5547E5E999A2BAF63A3B77D1325E7DD333BF8F63D0AA7AAEBA0A3319A64F7FE`,尚未注入或现场验证。
- 随后用户授权在新 PID 34760 上以修正 DLL 进行一次新的 UIN-only G3 callback 样本。Manifest 与指定实例门通过;CLI 返回 0,sequence 2,consumer 已启用,`g3s_done` 前各发送阶段均为 `ok:true`。90 秒后 callback 报告仍无 `consumer_result`、`consumer_worker_summary` 或 `consumer_orphan_callback`;目标仍存活/响应且加载 bridge。此结果只能表述为**未观察到 callback 诊断**;不能证明 QQ 没有调用 callback,也不能证明消息送达。该单次请求仍 delivery-unknown,不重试;OneBot 私聊路径仍在 native 前拒绝,因此不是 OneBot 端到端验收。记录: `local-evidence/n4-r4-g3send-34760-20261016/callback-sample-summary.json` 与同目录本地 raw logs。
- 该样本之后用户报告未收到对方回复并确认已正常重启 QQ;发出的正文是否显示未单独确认。只读盘点确认样本 PID 34760 已退出,当前 PID 34532 与 47560 均 alive/responding、加载同一冻结 wrapper hash 且未加载 Caligo。故 pinned image 已通过正常进程退出释放,但无回复和进程健康不能判定消息是否可见/送达,也不能判定 callback 是否发生。请求继续保持 **delivery unknown**,不重发;R2/R3 及 R4 状态不变。脱敏记录: `local-evidence/n4-r4-g3send-34760-20261016/post-restart-verification.json`。

## 已解契约(2026-10-09,反编译 r4-g3 phaseAE2)

### 注册侧 0x95d4e6(发送前由 25757ea 调用)
```
95d4e6(cookie*: [4]u32, spA: *shared_ptr, spB: *shared_ptr(record), cbCfg)
  若 A、B 均非空:
    cookieArr = {tag=2, cookie[0..16]}
    940588(&cookieArr, thunk@0x605d22) → 门控(bool)      # [更正] 第2参非字符串,
                                                          # 是 vcall thunk jmp [vptr+0xa8]
    若门控:
      key_fmt = 97b0f(fmt模板对象@0x3faa680)              # 模板 = "{} {}'{}\""(活内存实证)
                                                            # 格式化实参(反编译未保留,待运行时)
      key1 = 58e72(key_fmt, "svr"@0x4721cdf)              # 字符串拼接
      c707b6(A, cbCfg, key1)                              # [更正] 仅序列化 A + 日志(非注册!)
      key2 = 58e72(key_fmt, "sdk"位置@0x4791e1d)
      94e64a(*B, elements)                                # record → 元素表
      c707b6(elements, cbCfg, key2)                       # 仅日志
```
- cookie = **客户端自配的 16B msgUid**(25757ea 从发送上下文 +0x20 取)。
- A = 消息对象(vtbl+0x100 序列化;c707b6 日志会序列化它);
  B = MsgRecord(B vtbl slot0 有一次无参调用,产物被 key 覆盖,用途不明)。
- cbCfg = 9bef60 产物:对 record 形对象设特性键 0x17d5–0x17db、0x1a2d–0x1a30。
- **[更正] "注册"语义**:95d4e6 内没有消费者表写入;c707b6 全程只序列化+打日志
  (其 fmt 参考源路径 ".\modules\sys_env\publi…" 为日志文件名)。真正的
  消费者安装发生在 cbCfg(9bef60)的构造/设置路径 —— 注册点需重新定位。

### 响应侧 0x73bd6c(响应到达分发)
- param_1 = 消费者/处理器对象:{+8 日志器, +0x18 inner, +0x30 消费者
  调用指针, +0x38 verbose 标志}。
- 错误码提取(反编译实证,层级完整):
  1. 传输层参数(param_3)非 0 → errCode = param_3,errStr = 错误映射表
     查询(未命中 = "[MSF] Unknow Error!"@0x48df2da 区);
  2. resp 空 → errCode = **-1**(超时哨兵),errStr = "got empty response"
     (@0x47b1cd1 区);
  3. resp+0x40(int)非 0 → errCode = resp+0x40,errStr = string@resp+0x48;
  4. resp+0x80(字节)非 0 且 resp+0x60 非 0 → errCode = resp+0x60,errStr = resp+0x68;
  5. resp+0x80 非 0 且 resp+0x64 非 0 → errCode = resp+0x64,errStr = resp+0x68;
  6. 否则 errCode = 0(成功)。
- 终局调用(**合同闭合**):
  `(param_1+0x30)(param_1+0x18, 0x10, errCode:int, errStr:string, msgUid:string)`
  —— 门控 = 3fbab2(param_1+0x18)(消费者在场才调用);
  **msgUid = string@resp+0x38**(libc++ 串拷贝)。

## 实现路径(建议)
1.  bridge 内构造假 MsgRecord(vtable 0x3f51a78 + CPBMessageOpti 条目数组,
    最小 props:0x9c41/0x9c4a/0x9c55)与假消息对象 A(vtbl+0x100 序列化)。
2.  cbCfg/9bef60 路径重新定位消费者安装点(95d4e6 已证不是注册点)。
3.  以发送时用的 16B msgUid 为 cookie;请求体与 cookie 的关联字段待确认
    (响应按 resp+0x38 的 msgUid 串回带;cookie→msgUid 恒等假设待一次
    受控正例验证)。
4.  消费者入口(被 param_1+0x30 调用的函数)实现
    (inner, 0x10, errCode, errStr, msgUid) 签名,写入 g3s jsonl。

## 未决问题(实现期解;2026-10-09 静态收敛后剩三项)
- ~~940588 门控语义~~ → 收窄为:**slot 0xa8 背后的实现**(0x605d22 是
  vcall thunk,非字符串);需 941c96/该类 vtable 解码或运行时观测。
- **97b0f 的 fmt 实参**(3 个 {} 的实参来源;反编译丢失,需运行时或更深层解码)。
- ~~key 组合器格式~~ → key = fmt("{} {}'{}\"") + 后缀字面量(58e72 = 纯拼接,
  已闭合;后缀字节位置的精确字面量边界待定:"svr"/"sdk" 位置读数)。
- ~~请求 PB 中 msgUid 字段号~~ → 收窄为 cookie→msgUid 恒等验证(响应回带
  msgUid 串已实证;请求侧关联字段一次受控正例可定)。
- ~~消费者 A 实参 ABI~~ → **已闭合**:(inner, 0x10, errCode, errStr, msgUid)。
  剩余:消费者对象本身的 vtable/寿命(被 9bef60/cbCfg 持有方式)。

## 证据
- r4-g3 phaseAE2/functions/(95d4e6.c、73bd6c.c、9bef60.c、c707b6.c、940588.c、
  941b3a.c、97b0f.c、58e72.c、d3be4f.c = fmt 引擎、d3cb24.c)
- 25757ea.c(调用点上下文)
- @ 拒绝迭代:r20–r28b(g3-send-27624-r2x.jsonl;形状演进全记录)
- 活内存字符串读取(2026-10-09,实例 27108,外部 RPM 只读):
  `E:/stella/_reference/caligo-n3-resident-20261009/resp-consumer-strings.json`
  —— 0x3faa680 = "{} {}'{}\"";0x605d22 = thunk 字节(非串);
  "got empty response"/"[MSF] Unknow Error!" 文案实证。

## 2026-10-10 增补:调用方扫描 + 池观测替代路径

- **95d4e6 共 9 个直接调用方**(PE E8 字节扫描,wrapper.node):
  RVA 0x255e962/0x2564c56/0x2573e85/0x2575c0f/0x261cca0/0x261e20f/0x262b500/
  0x264a532/0x26537ec —— 全部落在 BaseMsgMgr/SendMsg 族(与 25757ea 同区),
  对应各消息类型发送路径。
- **73bd6c 调用链**:仅两个薄包装(≈0x73bd08/0x73c34c,内含直接调用 + 尾跳转),
  包装本身无直接调用方、无 .rdata 引用 → 真实分发经 vtable 间接;继续追需
  对包装函数区间做完整反编译(下一轮 Ghidra 定向任务)。
- **9bef60 语义更正**:不是"产 cbCfg",而是消费 (record, config) 给 record 设
  特性键(vtbl+0x38 set-int / +0x98 unset / +0xb0 set-obj;键 0x17d5–0x17db、
  0x1a2d/0x1a2e/0x1a30)——cbCfg 配置结构体的构造点仍在 25757ea 族上游。
- **池观测替代路径(诊断用,低风险)**:响应层日志串(got empty response/
  [MSF] Unknow Error! 等)经共享池可扫 —— 发送后扫池即可观测发送结局日志,
  不必先完成消费者安装;生产 OneBot 回执仍需正规消费者。
- 同期接收侧对照实验只作为 N3R10 的接收/序号校验参考,原始内容仍留在本机
  `_reference/caligo-n3-resident-20261009/`;它不包含响应消费者回调 UID,不能作为 R3
  发送请求与 CPB native identity 的关联证据。
