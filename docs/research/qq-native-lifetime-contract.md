# QQ 原生寿命/释放合同（G-LIFE / G-ABI）——固定 9.9.33-52230 静态证据 R3

日期：2026-10-08。对象与纪律同 [qq-native-thread-contract.md](qq-native-thread-contract.md)（RVA 寻址、只读静态证据、证据树 `E:/stella/_reference/qq-native-r3-p1-20261008/`）。本文覆盖计划 §7-P1 第 3 步（服务 pair 的完整 retain/release、析构辅助/allocator 对、callback operation 与 disposer）与第 4 步的寿命观察，是 [R2 qq-object-resolver.md](E:/stella/_reference/qq-native-r2-20261008/reports/qq-object-resolver.md) §7 的增量。

## 1. 控制块布局（`73CFA6` 构造）[verified]

```
offset 0x00 : vptr → vtable RVA 3F6F4D8（MSFService 块;写入点 0x73CFD8）
offset 0x08 : int32 强引用计数（uses）   —— getter/安装路径 LOCK 递增
offset 0x10 : int32 弱引用计数（weaks）  —— 弱成员/观察者 LOCK 递增
offset 0x18 : qword 托管对象指针（73CFA6 写入 param_2）
0x28 字节总量，分配器 B75000(0x28)
```

证据：[R2 phase4/functions/73cfa6.c](E:/stella/_reference/qq-native-r2-20261008/phase4/functions/73cfa6.c)、[R2 phase4/functions/1687c.c](E:/stella/_reference/qq-native-r2-20261008/phase4/functions/1687c.c)。

注意：这不是可直接跨 ABI 复用的 `std::shared_ptr` 声明——vtable 有 4 个槽（见 §2），与 MSVC 标准形态不同；Rust 层必须按" opaque 控制块 + 成对辅助函数"处理。

## 2. 控制块 vtable（`3F6F4D8`）四槽 [verified 形状，语义部分推断]

| 槽 | 目标 | 观察到的行为 |
|---|---|---|
| slot0 (`+0x00`) | `1F280` | `{ 调 28F0（purecall 垫片）; flag≠0 → B75029(self) }` —— 删除析构包装形态，基类实现为纯虚 |
| slot1 (`+0x08`) | `59F40` | （phaseA 导出为空体/极短，详见证据文件；未单独闭合） |
| slot2 (`+0x10`) | `73D040` | 按类型描述符 `185D8D590` 做 RTTI 匹配，命中返回 `self+0x20` —— "查询/转型到具体接口"形态 |
| slot3 (`+0x18`) | `1F30E` | 调 `1F348(out, self, 1)` —— 克隆/复制形态；**也是析构尾调用的槽**（见 §3） |

证据：[phaseA/functions/1f280.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseA/functions/1f280.c)、[phaseA/functions/73d040.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseA/functions/73d040.c)、[phaseD/functions/1f30e.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseD/functions/1f30e.c)。

## 3. 析构/释放链 [verified]

### 3.1 MSFService 单例对象

- vtable slot0 `7378A4` → `737A2E`（析构体；flag≠0 → `B75029`）。CoreService 对应 `74A156 → 74A220`（R2 已证，复核一致）。
- **观察到的递减模式**（以执行器单例析构 `9C15D6` 尾部为代表，[phaseD/functions/9c15d6.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseD/functions/9c15d6.c)）：
  1. 重置对象全部 vptr（析构签名动作）；
  2. 逐成员清理（含 `1DB2` 对 pair 成员的释放）；
  3. 取对象 `+0x18` 存的控制块指针；`LOCK` 递减 `*(int*)(ctrlblk+0x10)`（**弱计数**）；
  4. 旧值为 0 → 调用控制块 vtable **slot3**（`+0x18`）= 控制块自删除。

即：**弱计数归零 → 控制块自删**。强计数递减由 `1DB2` 完成（§3.4）；其前值为零时调用 control-block vtable `+0x08` 并进入 QQ deallocator。该虚函数与偏置/拥有者计数如何共同销毁托管对象仍未闭合；`2632` 是另一条 callback-manager dispose 路径。

### 3.2 获取（递增）位点全集 [verified]

| 位点 | 计数 | 证据 |
|---|---|---|
| getter `72DE38` / `74203C` 控制块非零时 | `+8` 强 | R2 §4（复核一致） |
| `1687C` 绑定（对象 `+0x40/+0x48` 内嵌句柄） | 新控制块 `+8` 强 | R2 §4 |
| `LoginRequestImpl` ctor `6E5B80` 两个 pair 成员 | `+0x10` **弱** | [phaseD/functions/6e5b80.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseD/functions/6e5b80.c) |
| TLS 安装器 `D32E5A` | `+8` 强 | [phaseD/functions/d32e5a.c](E:/stella/_reference/qq-native-r3-p1-20261008/phaseD/functions/d32e5a.c) |
| `5781C/57834` 替换式释放（发送路径请求对象） | 释放旧值 | R2 send contract §2 |

### 3.3 pair 移动/复制语义 [verified]

- `31C5F8(dst, src)`：非逐字段复制——调用 src 的 manager（`+0x10`）operation=0，然后把 src `+0x18` 改为空 `236A`、src `+0x10` 改为空 `2338`（move-like 管理协议；R2 §3 已证）。
- `D32E5A` 是 pair 的**强引用复制**（copy + 源清理）；`1DB2` 的强计数释放 ABI 见 §3.4；`2632` 是 callback manager operation-3 dispose，二者不是同一 pair-release 家族。

### 3.4 `1DB2` getter pair release [fixed-hash disassembly]

固定文件 `D:\Program Files\Tencent\QQNT\versions\9.9.33-52230\resources\app\wrapper.node` SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。只读复核的关键字节为：

```
RVA 0x1DB2: 48 8B 49 08 48 85 C9 0F 85 BD 08 00 00 C3
RVA 0x267C: ... B8 FF FF FF FF F0 0F C1 41 08 85 C0 74 06 ...
RVA 0x2695: 48 89 CE 48 8B 01 FF 50 08 ... E9 E5 86 75 00
```

`0x1DB2` 读取 `[pair+8]` 作为 control block；非空时进入 `0x267C`，对 `[ctrl+8]` 执行原子 `xadd(-1)`。`test eax,eax; je 0x2695` 的前值为零分支调用 control-block vtable `+0x08`，随后跳到 QQ deallocator `0xB75AD90`。因此 `1DB2` 的入参是 `{obj,ctrl}` pair 地址，签名可绑定为 `void(pair*)`；这是 getter pair 的释放辅助，不是 `0x2632` 的 manager operation-3 disposer。

构造点 `73CFA6` 将 `ctrl+8/+0x10` 初始化为零，getter `72DE38` 对 `ctrl+8` 加一。由此只能按 observed counter operation 成对调用 `1DB2`，不能把其末支简单解释为“本次 getter release 必然析构服务对象”，也不能读写控制块计数。固定 hash 上的 disassembly 与 getter/call-site 一致性支持 ABI 形状；它不证明 service shutdown 与异步 callback 已静止。

## 4. 执行器单例的寿命观察 [verified]

`9C0C68`（构造，静态存储 `750088`）：

- 构造即与控制块机制纠缠：成功路径 `D31AC0(*puStack_50, &handle, 3FAD8D0)` + `2EC8(puStack_90, ...)` 把内部 pair 存入对象成员；`9C13B5/9C13EF/9C142A/9C1562` 为成员 pair 的安装/初始化。
- **虚函数自调用**：`(**(code **)(*puVar4 + 8))()`（成员 vtable slot1）与 `(**(code **)(*plVar5 + 0x28))(plVar5, 9C2CB7(...))`（slot5）出现在构造流程中——对象在构造期即被注册进运行时。
- 失败分支：`4E91A6(puStack_50, 0)` 复位 + 日志——构造可失败，失败后存储被显式清零。
- 释放链：`9C2C6C`（删除包装）→ `9C15D6`（析构体，重置双 vptr → 逐成员 → §3.1 的弱减/自删尾）。

**含义**：任何持有该对象 pair 的自有代码必须能执行 §3 的完整对称释放；从 QQ 取回的 pair 若只丢弃控制块（SnowLuma 三处模式），对象永不析构且控制块泄漏（R2 §7 的静态契约缺口，本节给出其机制基础）。

## 5. LoginRequestImpl 寿命形状 [verified]

- 三 vptr 对象；`+0x18`、`+0x170` 为弱引用 pair 成员（弱计数获取）。
- 观察者通知（slot1 成功/slot2 状态码）都经 `3F70` 把 `this+0x20` 弱 pair **锁定为局部 strong**（lock 操作本身是控制块强递增），用完即 `1DB2` 释放。
- 通知在**当前线程同步发起**（无排队证据）；观察者实现与注册方未闭合（thread contract §7）。
- `6E5F3E`（第二处 vtable B 引用）= 析构侧候选 [verified 位置，语义待读]。

## 6. SnowLuma 调用侧缺口的机制定位（复核 R2 §7）

R2 的三条 getter 使用审计（`1760`/`15C0`/`4C40` 只取首 qword、不释放）在本轮机制下即：**取得 `+8` 强引用后永不递减**。对象侧析构链完整存在（§3/§4），因此缺口确在 SnowLuma 调用侧纪律，而非 QQ 对象不可释放。Caligo 自有实现取 pair 后必须：
1. 保存完整 `{obj, ctrlblk}`；
2. 已证 getter pair 可通过 `1DB2(pair*)` 成对释放；`2632` 属于 callback manager op3，不可替代 pair release；
3. 不以自有 allocator 释放 QQ 分配的控制块（`B75029` 是 QQ 的 free 包装）。

## 7. 已闭合 / 仍开放

**闭合**：控制块布局与 0x28 几何；强/弱两位点分离；五类获取位点；删除析构包装形态（`7378A4`/`74A156`/`9C2C6C`/`1F280` 同构）；析构体的 vptr 重置签名动作；弱归零→slot3 自删；LoginRequestImpl 弱成员与锁定式通知；TLS pair 强引用安装。

**开放（下一增量，均列计划未决问题）**：
1. control-block vtable slot1（`59F40`）的完整托管对象析构/弱计数关系，以及 pair counter 的完整拥有者模型；
2. callback manager `op=0` 的 clone、move wrapper 源清空及 `2632→op=3` dispose 有静态调用证据；`7328EA` 会销毁 nested callback 并递减 metadata strong count。op=1 的调用者/准确语义、其它 op 与重入/失败分支尚未穷尽；
3. `73BD6C` 到用户 invoker 是同步直接调用；固定样本上的用户 invoker ABI 为五参（见 §7.3），thread contract §10 将现场 callback 归于 QQ worker/dispatcher；目标实例的当前线程身份仍需按新会话核实；
4. 观察者注册方（谁构造 LoginRequestImpl、以何观察者 pair）——session-ready 充分条件；
5. dispatcher consumer 的 stop/cancel/drain、join/quiescence；没有证明断开/销毁后不再迟到回调。

### 7.1 Caligo callback code 的驻留边界（2026-10-11 离线实现）

- 在任何 Caligo callback context 可发布前，`consumer_probe::leak_ctx` 通过 `GetModuleHandleExW(FROM_ADDRESS | PIN, consumer_invoke)` 固定包含 delegate 函数指针的 bridge image；失败则返回不可用，调用方在 native submit 前停止。
- PIN 由 Windows loader 保证模块保持映射直到宿主进程终止，即使收到 `FreeLibrary`；调用不保存或释放返回句柄。[Microsoft API contract](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getmodulehandleexw)
- `caligo_native_resident_run` 与 `caligo_g3_send_run` 都先经该 context gate 再构造 callback delegate。常驻 generation 停止时先封闭回调准入，等待已进入的回调离开；之后的迟到 callback 只增加 `late_callbacks_rejected` 审计计数，不读取错误诊断指针，也不进入 256 项响应槽。没有已登记 generation 的回调按 orphan 计数，也不进入共享槽。对应离线测试验证上述隔离行为。
- `native_resident_run` 保留 pump、receive 与 daemon worker 句柄；stop 后有界 join，超时写失败阶段并返回非零。此项只证明 Caligo 自有 worker 的停止结果，不能证明 QQ consumer 已停止、不能取消 QQ 已排队工作，也不证明最终结果已持久交付。
- 这些改动只消除了 bridge 被卸载后 callback 指针悬空、停止后的持续迟到调用占满共享响应槽，以及把固定 sleep 当作自有 worker 已退出的风险。等待的是 Caligo 已准入回调并 join 自有 worker，不是 QQ dispatcher quiescence；owner thread、op 完整性、原生 stop/cancel/drain/join/quiescence 与历史 AV 继续开放。不得把 PIN/本地准入栅栏解释成 G-THREAD 或现场准入通过。

### 7.2 Callback manager operation dispatch 静态补充（2026-10-13）

针对本机冻结样本 `E:\stella\_reference\qq-native-r2-20261008\inputs\wrapper.node`，SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。使用 Ghidra 12.1.4 `-noanalysis`，由固定哈希脚本定向导出 `73C2AA`、`73BC5C` 两个 manager；再读取它们的相对跳转表并反汇编八个 operation 入口。该过程只处理磁盘文件，不加载或调用 QQ 进程。

| manager 布局 | op | 跳转目标 | 静态可见行为 |
|---|---:|---:|---|
| inline `73BC5C` | 0 | `73BCBE` | 经 `4D3588`、`73BBF8` closure 路径处理并调用 `7328EA`；与 `31C5F8` 的 op=0 调用及源 pair 清空相容，准确调用语义仍以已见 move-like 用法为限。 |
| inline `73BC5C` | 1 | `73BCF3` | 以 `R8/R9` 调 `4D3588` 后返回；该路径没有在此函数内改写 manager/invoker 字段。准确调用者与请求语义未定。 |
| inline `73BC5C` | 2 / 3 | `73BC7F` | 调 `4D3588` 与 `7328EA`；仅 op=2 随后把 manager/invoker 改为空函数 `72D596/72D5C8`，op=3 不做该改写。op=3 的已知外层调用仍是 `2632`。 |
| heap `73C2AA` | 0 | `73C30F` | 将 `R8` 指向的 holder 内容写入输出，并设置 heap manager/invoker `73C2AA/73C34C`；`31C5F8` 随后清空源 pair。 |
| heap `73C2AA` | 1 | `73C32A` | 直接函数尾返回，没有在该分支内读写 closure 字段；调用语义未定。 |
| heap `73C2AA` | 2 / 3 | `73C2DA` | 经 `7328EA` 清理 closure，随后以 closure 指针与 `0x40` 参数调用 `1FD0 → 1FE0 → B75029`；与下述 `24C0 → B75000` 的 0x40 heap fallback 分配配对。仅 op=2 随后重置 manager/invoker，op=3 不重置。 |

两 manager 都直接用 operation 值索引跳转表；入口没有看到范围检查。这里确认的是固定样本中 0–3 的机器码分支，不证明其它值可用，也不证明所有调用点只传这些值。`op=0` 的 move-like 用法、`op=3` 的 dispose 用法有可定位包装调用；`op=1` 的调用者和完整语义、op=2 的调用合同及所有调用点仍未闭合。下一步不得主动调用未知 operation。

2026-10-13 allocator follow-up：`73BBF8` 在 `4D3588` 未返回 arena 地址时调用 `24C0(0x40)`，将 closure 存入返回的 heap block，并安装 heap manager/invoker；`24C0` 跳到 `B75000`。`73C2AA` 的 op=2/3 分支从 holder 取同一 closure 指针，先调用 `7328EA` 清理内容，再以 `(closure, 0x40)` 调 `1FD0`；`1FD0 → 1FE0 → B75029`。这闭合固定样本中 heap-fallback closure 的同尺寸 QQ wrapper 分配/释放配对。它不证明 wrapper 内部 allocator 细节、arena 路径寿命、op=1/op=2 外部调用合同、op=3 可重复调用，也不闭合线程/quiescence。定向导出及 hash 在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\allocator-helper-analysis\followup-20261013\`；只反汇编固定磁盘样本，未加载或调用 QQ。

原始本机 manager 输出位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-dispatch\`：`dispatch.tsv`、`range_73bc5c.asm`、`range_73c2aa.asm`、对应反编译文件及 `target-hash.txt`。allocator follow-up 的跳板反汇编与 hash 位于上述 `allocator-helper-analysis\followup-20261013\`。这些静态增量不闭合 op=1/op=2 完整调用者与语义、owner/consumer thread、QQ dispatcher stop/cancel/drain/quiescence、historical AV 或现场准入。

### 7.3 `73BD6C` 用户 invoker 五参 ABI 与 Rust 对齐（2026-10-13）

对象仍是 §7.2 固定磁盘样本及相同 SHA-256。Ghidra 12.1.4 `-noanalysis` 导出位于本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\callback-contract\assembly\73bd6c.txt`。在调用 `[RSI+0x30]` 前，机器码将 `RDI` 写入调用者 `[RSP+0x20]`，并设置 `RCX=RSI+0x18`、`RDX=0x10`、`R8D=EBX`、`R9=RBP-0x28` 后 `CALL RAX`；调用者在此 call 前未设置 `[RSP+0x28]`。按 Windows x64 ABI，这是一条五参调用：`(inner, 0x10, errCode, errStr, responseUid)`，没有被证明的第六参。该局部 string-like 值在回调返回后由 `0x290A` 清理，与 `response-consumer-scope.md` 记录的 `resp+0x38` msgUid 形状相容，但本段不证明其编码细节或与 Caligo request cookie 的身份对应。

因此原 `consumer_invoke` 六参声明中的 `extra` 没有对应的已传参；继续读取它会把未定义的调用者栈内容误作 span。Rust 入口现按五参声明，保留第五参为 opaque response UID pointer，不解引用；错误码与错误文案仍进入既有有界槽，span-shape 猜测字段/读取已移除。测试用函数指针类型锁定五参 ABI，并以不可读第五参验证捕获路径不依赖它。离线编译/测试不证明 QQ 现场回调已发生，也不证明 response UID 与发送 cookie 相等。

本节关闭的是固定样本 `73BD6C` 用户 invoker 参数个数与源码声明不匹配这一 ABI 子项。callback manager op 0–3 的完整调用合同、owner/consumer thread、原生 stop/cancel/drain/quiescence、稳定 native identity 及历史 `0xC0000005` 仍开放，R4 准入不变。

### 7.4 Callback manager consumer callers follow-up（2026-10-13，静态）

继续针对同一冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）做 Ghidra 12.1.4 `-noanalysis` 定向反汇编/反编译，并将直接调用点限定到 PE `.pdata` 函数范围。全程只读磁盘样本，没有启动、读取或操作 QQ 进程。

- `0x54CE00` 是 16 字节包装对象的 assignment helper：先对目标调用 op=3，再对来源调用 op=1，参数包括 size `0x10` 与目标上下文。
- `0x9B31C8` 先对 `source + 0x10` 调用 op=4 输出查询 helper `0x3FCE5E`（其“callback presence”语义见 §7.6 勘误），然后经 `0x54CE00` 复制至目标，最后通过已知 wrapper `0x2632` 对 source 调用 op=3。这直接支持该共享 callback-manager 接口的复制/assignment 与来源释放用法。`0x744D7E` 也在 assignment 后对 source 调用 op=3；`0x31FA9D4` 在复制较大对象时两次调用 helper，处理相邻的两个 16 字节成员。
- `0x3BC545C` 以 op=1 将 size `0x10` 的 source wrapper 复制到局部存储，再把副本传给 `0x3BC6298`。其上游 `0x3BC4C18`、`0x3BC4D84` 从请求对象 `+0xA0` 字段取该对象，并将副本传入下游 callback/request helper；这是强 callback-field 候选，但具体 manager table 仍未解析。
- 排除 `0x970E2A`（其调用方把 op=2 返回字节当布尔值）、`0x982F4E`（op=1/op=2 返回字节进入配置序列化字段）和 `0x340CA75`（把 op=1 返回值当布尔值）。这些返回形状不符合已知 callback manager op=1/op=2 的 void 路径。`0x3225B8A`、`0x33FCBA4` 是另一 `+0x68` property manager 的属性序列化调用；`0x34A156E` 的 0x20 步长数组 op=1 copy 仍因容器/成员所有权未知而未定类。

本次有界 `E8 rel32` 扫描在 `.pdata` 函数范围内找到 42 个目标匹配；它不含完整间接引用或数据流引用，不能据此称为完整 xref，也不能从缺项推出“没有调用”。原始输出和固定哈希记录位于本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\` 下的 `manager-callers-followup-20261013\`、`caller-refs-20261013\`、`candidate-context-20261013\` 与 `candidate-context-followup-20261013\`，未入库。

这补强了共享接口的 op=1/op=3 consumer 用法；仍未证明这些对象确实使用 inline 表 `0x3F7052C` 或 heap 表 `0x3F70540`，也未穷尽 op=1/op=2 调用者或解析 op=2 外部合同。arena 分支、owner/consumer thread、QQ dispatcher stop/cancel/drain/quiescence 和历史 AV 根因继续开放；R4 不准入。

### 7.5 Callback manager table owner 与 op=2 bounded caller screen（2026-10-14，静态）

再次核验安装磁盘样本 `D:\Program Files\Tencent\QQNT\versions\9.9.33-52230\resources\app\wrapper.node`，SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。筛查器只读取 PE 可执行节与 `.pdata` 函数范围，不启动或读取 QQ 进程、不调用函数、不操作 SnowLuma；脚本与地址级输出留在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\op2-caller-screen-20261014\`。

- 限定 `.pdata` 中的直接 `E8 rel32` 调用后，冻结目标集合共 41 个匹配；`73BBF8` 仅见外部直接调用 `0x732A80 → 0x73BBF8`，另一个是 inline manager 的 op=0 路径 `0x73BC5C+0x81 → 0x73BBF8`。`73BBF8` 在 `4D3588` 返回 arena 地址时选择 `{manager=73BC5C, invoker=73BD08}`；未返回时以 `24C0(0x40)` 分配并选择 `{manager=73C2AA, invoker=73C34C}`。这把两种 manager table 形状连接到共同的 builder，但不证明 arena 分支对象寿命或下游 consumer 的停止/排空。
- 在全部可执行节内筛选精确 `mov edx,2` 编码并映入 `.pdata`，得到 5,496 个候选指令、4,801 个函数；限定同一基本块里的首个调用且调用形状为 `[reg+0x10]` 后有 12 个候选。它们显示为对象虚表调用形状（其中 `0x970E2A`、`0x982F4E` 的返回字节被保存/序列化），没有一个能以已知 manager slot 调用形状或具体对象字段，绑定到 `73BC5C/73C2AA` op=2。原有排除仍适用。
- 该筛查仅覆盖固定 immediate 编码、有限指令形状与直接调用；间接引用、不同参数准备/编码、未识别 wrapper 及数据流路径仍可能存在。结果只支持“本次 bounded screen 未找到可归属的 op=2 caller”，不支持“没有 op=2 caller”，也不闭合完整调用者集合或外部合同。

可复现材料：`r2-op2-caller-screen.py` 与 `r2-op2-caller-screen.txt`（同目录）。新证据仍不闭合 op=1/op=2 全调用者、arena 寿命、callback owner/consumer thread、QQ stop/cancel/drain/quiescence、历史 `0xC0000005` 或现场准入；R4 继续不准入。

### 7.6 Callback manager op=4 与 op=2 ABI 调用点补充（2026-10-14，静态）

本节复核与 §7.5 相同的冻结磁盘 `wrapper.node`，SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；只读 PE、`.pdata` 和反汇编，没有访问 QQ 进程或操作 SnowLuma。

- 两张已知 manager 相对跳表地址分别是 `0x3F7052C`、`0x3F70540`，相差 `0x14`。按已验证的相对位移解码 `table+0x10`，inline `0x73BC5C` 的 op=4 指向 `0x73BCB0`，heap `0x73C2AA` 的 op=4 指向 `0x73C309`；前者执行 `and qword ptr [rbx],0`，后者执行 `and qword ptr [rax],0`，二者都把第五参数指向的 qword 清零。故已知两张表的可证分支范围是 op=0–4，而不是只到 op=3；分支没有范围检查，不能把 op=5 或其它越界值当成有效操作。
- `0x3FCE5E` 加载传入 pair 的首 qword 并以 `(RCX=0, EDX=4, R8=0, R9=0, fifth=&out)` 调 manager，之后返回 `out != 0`。对上述两张表，op=4 把 `out` 清零，因此 helper 返回 false。此前 §7.4 依调用位置把它称为“presence helper”，语义命名过强；本节将其校正为 manager op=4 输出查询 helper。它是否表示 callback 是否存在，不能从该 helper/两张表的 op=4 实现推出。
- 对全部可执行节中的精确 `mov edx,2`、`.pdata` 函数范围内的同基本块首个间接调用，加入轻量寄存器来源归一化；要求 RCX 等于间接 manager 函数指针槽地址、R9D=`0x10`，且 op=2 返回值在紧邻指令内未被观察。此 bounded ABI 形状筛选得到 9 个调用点、分布于 7 个函数：`0x9C50FF/0x9C511A/0x9C5135`、`0x9E13A7`、`0x3456CE6`、`0x34576E5`、`0x35140D5`、`0x3B36599`、`0x3DD3ECD`。其调用均把 RCX 指向与间接目标相同的 pair/manager slot，并传 `R9=0x10`；局部上下文与清理/收尾路径相容。两个调用点 `0x35140D5` 和 `0x9E13A7` 还会在同一字段上调用 `0x3FCE5E` 的包装（后者经 `0x3FBAB2` 转调），然后在各自条件分支中 op=2。
- 这 9 个是高置信 manager-ABI-shaped op=2 caller 候选，较 §7.5 的 `[reg+0x10]` 过滤有所扩展；结合 pair-slot、16-byte size 与相邻 op=4 调用，足以把 op=2 调查推进到具体静态调用位置。但间接函数指针的动态值未解析，尚不能逐一断言它们运行时指向 `0x73BC5C` 或 `0x73C2AA`；寄存器归一化与近邻返回值检查也不是全路径数据流/完整 xref。不得据此推断调用时机、manager 外层 owner、线程、closure 寿命或 dispatcher quiescence。

可复现筛查器及地址级输出仅存本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\op2-manager-shape-screen-20261014\r2-op2-manager-shape-screen.py` / `.txt`。此增量没有改变 R2/R3/R4 准入：consumer/owner thread、manager 外层 teardown、QQ stop/cancel/drain/quiescence、历史 `0xC0000005`、R3 stable identity 仍开放；R4 继续不准入。

### 7.7 op=2 候选代码引用的 RTTI/vtable 复核（2026-10-14，静态）

对 §7.6 中 `0x9C509C`、`0x345763E`、`0x3456C1C` 三个函数，复核此前记录的 `.rdata` qword 代码地址，并沿 RTTI/vtable 与 vptr 写入路径追查。样本仍为磁盘 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；无 QQ 进程读取、函数调用或 SnowLuma 操作。最终脚本与原始输出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\op2-manager-shape-screen-20261014\r2-op2-vtable-membership-20261014.py/.txt` 和同目录 `r2-op2-owner-method-context-20261014.py/.txt`。

- 扫描 `.rdata` 中满足 image-relative signature/self-RVA、有效 type descriptor 与 class hierarchy descriptor 的 COL，并以 COL 指针定位 vtable，再按下一个有效 header 与连续 `.text` 函数地址边界限制 slot。总计发现 15,412 个有效 COL / 15,410 个 vtable header；已知 `ThreadTaskManagerImpl` vtable `0x4260448` 被正确识别为 parser 正向对照。此前三个 qword 位于 vtable 中间，前一 qword 是上一 virtual slot，而不是 COL；只检查“前一 qword 是否 COL”会误判 vtable 起点。

| op=2-shaped 函数 | 有效 vtable / slot | RTTI class descriptor | vptr 写入点 |
|---|---|---|---|
| `0x9C509C` | RVA `0x3FADD38`, slot 5 | `.?AVFileDragManagerWin@nt@@`，COL RVA `0x3FADD70` | `0x9C47E6` 计算表地址，`0x9C47ED` 写入 `[rcx]` |
| `0x345763E` | RVA `0x43E7528`, slot 3 | `.?AVIgnoreBodyHttpRequestHandler@httpx@@`，COL RVA `0x43E75B0` | `0x345747D` 计算表地址，`0x3457484` 写入 `[rcx+8]` |
| `0x3456C1C` | RVA `0x43E73B8`, slot 3 | `.?AVSaveToStringHttpRequestHandler@httpx@@`，COL RVA `0x43E74E0` | `0x34569A6/0x3456D4C` 计算表地址，后续写入 `[rcx+8]` |

- 这些 RTTI/vtable 结果确定的是 op=2-shaped 函数所属的 enclosing C++ class/method slot，不是间接 manager field 的目标类型。`0x9C509C` 对 `[this+0x30]`、`[this+0x50]`、`[this+0x70]` 分别执行 op=2；`0x345763E` 与 `0x3456C1C` 则从 `0x123528` 返回对象的 `+0x38/+0x48` manager slot 调 op=2。动态调用值仍未逐一绑定到 `0x73BC5C/0x73C2AA`。
- `0x345745A` 的 vptr 初始化路径还从输入对象 `+0x10` 取间接 manager target，以 `EDX=1`、`R9=0x10` 调用，目标 pair 地址在 `this+0x20`；这是一个新的 op=1-shaped copy/initialization 点，但其动态 target 仍未知。`0x3456986 → 0x3456A0A` 的 base 初始化另以 op=0 调输入 manager；这些用法不能充当三个 op=2 slot 的具体动态 table 证明。候选 handler 的构造/析构 vptr 写入已找到，manager/pair 初始化所有权仍未闭合。
- 可复现脚本/输出：本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\op2-manager-shape-screen-20261014\r2-op2-vtable-membership-20261014.py/.txt` 与 `r2-op2-owner-method-context-20261014.py/.txt`。脚本在读样本前校验固定 hash。COL 扫描和常见 RIP-relative xref 仍有编码/解析边界，不构成全 binary xref、调用时序、manager 动态值或寿命/线程/quiescence 证明。

### 7.8 `HttpConnection+0xA0` callback wrapper owner（2026-10-14，静态）

重新核验磁盘 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。只读 PE、`.pdata` 与目标函数反汇编；没有启动/读取 QQ 进程、调用原生函数、发送消息或操作 SnowLuma。

- `0x3BC2C3A` 与 `0x3BC3082` 中的 RIP-relative vptr 都指向 `.rdata` RVA `0x458D3B8`；前置 COL RVA `0x458D4C0` 的 type descriptor 是 `.?AVHttpConnection@httpx@@`。vtable slot 0 的 `0x3BC550C` 调用 `0x3BC3082` 清理主体。由此将该字段 owner 从此前的 request-object 候选准确收敛为 `httpx::HttpConnection`。
- 构造函数 `0x3BC2C3A` 将识别为空 wrapper 哨兵的 manager/invoker 地址 `0x2338` / `0x236A` 写入 `this+0xB0/+0xB8`，并把 `this+0xA0` 传给该 manager 的 op=3、`R9=0x10` 路径。析构主体 `0x3BC3082` 对 `this+0xA0` 调用 `0x2632` dispose，构成字段初始化与释放的配对证据。
- `0x3BC456A` 的状态分发分别进入 `0x3BC4C18`（设置状态 6）和 `0x3BC4D84`（设置状态 8）；两者都把 `this+0xA0` 交给 `0x3BC545C`。后者从该 wrapper 的 `+0x10` manager slot 发起 op=1、16-byte copy，再把副本传给下游 HTTP request/callback helper。因此，现可确认 `HttpConnection+0xA0` 是 manager-managed callback wrapper storage；这不表示任一运行实例当前持有非空 callback。
- 在本节初次追查时尚未定位 empty→non-empty writer；后续固定-hash 复核已在 §7.9 找到 ctor 赋值路径及该字段局部 inline/heap manager/invoker 目标。此结论不将局部 helper 误绑到 `0x73BC5C/0x73C2AA` 全局表；全局表的完整 caller/ABI、arena 分支寿命、manager/consumer owner thread、外层 teardown、QQ dispatcher stop/cancel/drain/quiescence、历史 `0xC0000005` 和 R3 stable identity 继续开放，R4 仍不准入。

可复现脚本/文本输出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\r2-httpconnection-callback-field-owner-20261014.py/.txt`。该脚本在读取前校验固定 hash；只含本机静态地址与 RTTI，不入库。

### 7.9 `HttpConnection+0xA0` callback capture 安装路径（2026-10-14，静态）

继续使用 §7.8 的冻结磁盘 `wrapper.node` 和 SHA-256；脚本先验 hash，只读 PE、`.pdata`、反汇编与 RTTI，没有访问 QQ 进程或操作 SnowLuma。

- 构造路径可沿直接调用串复核：`0x3458A18` 在 `R8D==0` 分支调用 `0x3458B00`（`0x3458A5D`）；该工厂分配 `0x168` 对象并调用 `0x3BC2C3A`（`0x3458B61`）。有界 `.text` rel32 / `.pdata` 扫描只发现这一个直接 ctor call site；间接调用、取址调用及扫描解析边界不在此结论内。
- `0x3BC2C3A` 先把 empty manager/invoker sentinel 放入 `this+0xB0/+0xB8`，再以 op=3、size `0x10` 初始化 `this+0xA0`。随后 `0x3BC3008(this+0x158, this)` 经 `0x3BC66B5 → 0x3BC66EF` 构造 capture pair。`0x3BC66EF` 分配并初始化 RTTI 为 `__shared_ptr_emplace<WeakWrapper<HttpConnection>>` 的对象；其 vptr RVA `0x458D5F8` 的 COL RVA 为 `0x458D620`。helper 清零 control-block 计数，把 `this` 写到对象 `+0x18`，输出 `{object+0x18, object}`；ctor 对 control block `+0x10` 做强引用递增。因此已见成功构造路径生成的是指向该 `HttpConnection` 的 weak-wrapper capture pair。
- ctor 从 `this+0x158/+0x160` 复制该 pair 到临时 16-byte wrapper并增加 control-block 强计数，然后调用 `0x3BC678A(temp, &this->manager_invoker, this+0xA0, 0x10)`。该 helper 在 `0x40634` 的 inline-storage 路径成功时安装 manager `0x3BC67F8` / invoker `0x3BC6436`；失败时经 `0x24C0(0x10)` 分配 heap fallback 并安装 `0x3BC692A` / `0x3BC6576`。两条路径都把 pair 复制进目标存储并清空临时 pair。由此，成功完成该 ctor 的路径会把 empty `+0xA0` wrapper 填成持有 HttpConnection weak-wrapper capture 的非空 wrapper；其动态 manager/invoker 目标已绑定到这组本地 inline/heap helper，不是 `0x73BC5C/0x73C2AA` 两张全局 manager 表。
- 状态 6 与状态 8 路径经 `0x3BC545C` clone 此 wrapper，再通过 `this+0x38` handler 的虚表 `+0x10` 调用转交 callback wrapper 和状态数据。当前只确定调用形状；具体 handler 实现、实际 consumer 线程、外层对象 teardown，以及 QQ dispatcher stop/cancel/drain/quiescence 仍未证。该字段路径不证明某个现场实例已构造、callback 已执行或结果已送达。
- 上游直接调用点筛查还找到 `0x3458736→0x3458A18` 与 `0x34592DC→0x3458A18`。前一 caller 在 `[this+0x18]==0` 分支 move 出自己的 `this+0x38` 对象并以 `EDX=0/R8D=0` 进入创建分支；该对象经 `0x3458B00` 被转交为新 `HttpConnection+0x38` handler。另一 caller `0x345927C` 复制一组 owner pair，再调用 `0x5CD68C`；该 helper 将输入 pair 交给 `0x3F70`，复制临时 pair 中的对象指针后释放临时与输入副本，形状与 weak-reference lock 一致，但 pair 的具体静态类型尚无 RTTI 证明。非空返回指针被传入 `0x3458A18`；该返回对象的动态类与第二条 `HttpConnection+0x38` handler 类型仍未闭合。

可复现脚本与地址级文本保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\`，包括 `r2-httpconnection-write-sites-20261014.py/.txt`、`r2-httpconnection-owner-call-context-20261014.py/.txt`、`r2-httpconnection-nested-manager-helpers-20261014.py/.txt`、`r2-httpconnection-inner-lambda-20261014.py/.txt`、`r2-httpconnection-parent-callers-20261014.py/.txt` 与 `r2-httpconnection-parent-callsite-context-20261014.py/.txt`。全映像扫描的 immediate/xref 覆盖和反汇编边界有限，不能据未命中推断无其它动态构造路径。

### 7.10 `HttpConnection+0x38` 的已接受 socket 回调路径（2026-10-14，静态）

重新对冻结磁盘 `wrapper.node` 做单次 hash 校验，SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。只读 `.pdata`、反汇编与 RTTI/vtable；没有访问 QQ 进程、调用原生函数、发送消息或操作 SnowLuma。

- 创建与接受链可静态相连：`0x2A0690A → 0x2A08062` 分配 0x30 字节 listener 并经 `0xCADD64 → 0xCADD8A` 构造；随后 `0x2A080BD` 把对象交给 `HttpServer` ctor `0x3457D10`，该 ctor 将它写入 `HttpServer+0x10`。构造/vptr 与 RTTI 确认 listener 是 `TCPServerSocket@xpng`（vtable RVA `0x3FE48E8`、COL `0x3FE4920`）。`0x3458614` 从该字段取对象，经 vtable `+0x28` 发起 accept，并把输出 pair 指向 `HttpServer+0x38`；成功时进入 `0x3458736`。`TCPServerSocket` slot 5 (`+0x28`) 是 `0xCADF0A`，转调 slot 4 (`+0x20`, `0xCADF7E`)；该方法以 `TCPServerSocket+0x8` 的 vtable `+0x20` 接受底层连接。
- `TCPServerSocket+0x8` 的对象由 `0x217C624` 分配并经 `0x31F66EC` 构造；vptr RVA `0x43B5E18`、COL `0x43B5EB0` 的 RTTI 为 `TCPSocketLibuv@xpng`。其 slot 4 (`+0x20`) 为 accept 方法 `0x31F6CF6`。成功分支通过 `0x31F6B91 → 0x31F66EC` 构造另一个同类 0x38 字节对象，再经 `0x31F6BBC` 组成 shared pair 返回给 `TCPServerSocket+0x10`。`0xCAE0DC` 随后消费该 pair，分配 0xA0 字节对象并调用 `0xCACD38`；该 ctor 写入 `TCPClientSocket@xpng` vtable（RVA `0x3FE46D8`、COL `0x3FE4740`），将底层 pair 存入自己的 `+0x8`，并把新对象交回 `HttpServer+0x38`。该 pair 经 `0x3458736 → 0x3458A18 → 0x3458B00 → 0x3BC2C3A` 后成为 `HttpConnection+0x38`。因此本条 accepted-connection 路径上的 `HttpConnection+0x38` 动态类型为 `TCPClientSocket@xpng`，其 `+0x8` 为 `TCPSocketLibuv@xpng`。
- `HttpConnection` 状态 6/8 (`0x3BC4C18` / `0x3BC4D84`) 均以 `this+0x38` 的 vtable `+0x10` 转交复制的 callback wrapper 与状态上下文。`TCPClientSocket` slot 2 (`+0x10`) 为 `0xCAD3B8`；它调用 `+0x8` 对象 vtable `+0x40`，即 `TCPSocketLibuv` slot 8 `0x31F729E`，在内层调用返回 `-1` 时走 `0x3FBB22` 辅助路径。`0x31F729E` 检查对象状态并传入本地 callback entry 地址至 helper `0x3EB5090`；仅能确认这个静态调用形状，函数名/回调线程/结果合同均未确定。此为通用 xpng/libuv socket 层的回调转交，不是 QQ callback manager 的语义证据。
- 勘误（2026-10-14 follow-up）：上述 direct `E8 rel32` / `.rdata` absolute-qword 初筛漏掉 RIP-relative 引用；后续按有效 `.pdata` 函数边界复扫，在 `0x3459127`、`0x3459144`、`0x34593EF` 找到两个 invoker 的入口引用。`0x42F2B` helper 的 RTTI 语义仍未绑定；替代 owner 与分支证据见 §7.11–§7.12。初筛不能据此推断无 caller。
- 路径边界：`0x34592DC`（caller `0x345927C`）是另一个 `0x3458A18` 调用点；后续 §7.11 已将 captured owner 的静态类型收敛到 `WeakWrapper<HttpServer@httpx>`。factory 以 `R8D` 作零/非零路由 gate；寄存器级分支续查见 §7.12，业务语义与节点类型仍 unknown。本节已接受路径属于通用 `httpx`/`xpng` HTTP socket 层，不证明它就是 QQ 发送响应消费者，也不绑定到全局 manager tables `0x73BC5C/0x73C2AA`；没有现场 callback 执行证据。

复现脚本与地址级导出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\r2-httpconnection-tcpclient-chain-20261014.py/.txt`、`r2-tcpserver-inner-acceptor-rtti-20261014.py/.txt`、`r2-httpserver-listener-ctor-xrefs-20261014.py/.txt` 与替代 wrapper 筛查 `r2-alternative-parent-xrefs-20261014.py/.txt`、`r2-alternative-wrapper-xref-screen-20261014.py/.txt`。脚本读取样本前核验 hash；地址范围与 RTTI/vtable 输出包含 factory/ctor、accept 和 callback-forwarding slots。这里只闭合已接受连接的通用 HTTP/socket 路径；实际 QQ callback consumer/全局 manager 身份、owner/consumer 线程、外层 teardown、QQ stop/cancel/drain/quiescence 和历史 AV 根因仍开放。

### 7.11 `HttpServer` 替代 callback wrapper 的弱 owner（2026-10-14，静态）

重新核对同一冻结磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。本节仅为静态 `.pdata`、RIP-relative xref、ctor 字段与 RTTI 证据，不访问运行中的 QQ。

- `HttpServer` ctor `0x3457D10` 在 `this` 与 `this+0x8` 写入的两个 vptr 分别指向 RVA `0x43E75D0`/`0x43E75E8`；各自 COL 的 signature 为 1、self RVA 与 COL 相等，RTTI 均为 `.?AVHttpServer@httpx@@`。ctor 以 `rcx=this+0xE0, rdx=this` 调 `0x3457FDA` 建立该字段的 owner pair。
- `0x3457FDA → 0x345947A → 0x34594B4` 建立控制块；`0x34594B4` 在 `0x34594E0` 写入的 vtable RVA `0x43E7738`，其有效 COL RVA `0x43E7760` RTTI 是 `__shared_ptr_emplace<WeakWrapper<HttpServer@httpx>>`。构造体把源 `HttpServer this` 写到 control block `+0x18`，形成 `{control+0x18, control}` 并安装到 `HttpServer+0xE0/+0xE8`。这是 wrapper 的静态 captured owner type 证据；它不排除运行实例来自派生类型。
- accept 方法 `0x3458614` 以同一 `this` 调 `0x3458736`。后者复制 `this+0xE0/+0xE8` pair、附加 `+0x5C` 生成的 DWORD，再调用 `0x3459100`。该 helper 按 inline/heap 分支分别安装 local manager/invoker `0x3459174/0x3459220` 与 `0x3459384/0x3459426`，并复制捕获 pair 与 DWORD；它们不是全局 manager tables `0x73BC5C/0x73C2AA`。
- 全 `.pdata` 边界 RIP-relative scan 在 `0x3459127` 找到 wrapper `0x3459220` 的地址引用，在 `0x3459144`/`0x34593EF` 找到 `0x3459426` 的地址引用；两个 invoker 都进入 `0x345927C`。该函数用 `0x5CD68C` 对 captured weak owner 做 lock-shaped 操作后，把返回的 HttpServer owner、state 和 pair 传给共同 factory `0x3458A18`。该 factory 的 state 0 路径进入 `0x3458B00`；非零 state 走 `0x3459746`，不同分支不合并成一个 handler 类型结论。
- 因而前节 `0x34592DC` 路径的 captured owner 已静态归属 `HttpServer@httpx`，不再是无类型 weak-return 候选；但本结论只解释 generic HTTP Server 的 callback owner/状态分派，不证明 TCP callback 的实际执行线程、任一 `HttpConnection+0x38` 在所有状态下的动态 handler、QQ response consumer、原生 manager 线程/释放合同或 teardown/quiescence。

可复现脚本与导出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\`：`r2-alternative-pdata-rip-xrefs-20261014.py/.txt`、`r2-alternative-wrapper-registration-context-20261014.py/.txt`、`r2-httpserver-class-rtti-validated-20261014.py/.txt`、`r2-httpserver-weak-capture-and-state-route-20261014.py/.txt`、`r2-httpserver-weak-pair-construction-20261014.py/.txt` 与 `r2-httpserver-weak-wrapper-rtti-20261014.py`。每个读取脚本先核验固定 SHA-256；RIP-relative 筛查以有效 `.pdata` 函数边界为解码输入。静态命中仍不替代现场 callback 或 R4 准入。

### 7.13 `HttpServer+0x20` 注册 key 的写入与回流（2026-10-14，静态）

继续读取同一冻结磁盘 `wrapper.node`，SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。本节仅描述 generic `HttpServer@httpx` 的机器级 key 路径，不把它映射成 Caligo request ID、QQ native callback 或业务回执。

- `0x3458614` 在 listener 虚槽 `+0x28` 返回非 `-1` 后调用 `0x3458736`。该方法先按 `EDX` 符号位分支；成功路径从 `HttpServer+0x18` 取源对象，递增 `DWORD [this+0x5C]`，并将新 DWORD 作为 key 交给 `0x34589B8`，其容器参数是 `this+0x20`。因此 map key 的静态来源已收敛为 HttpServer 自增值；它不是调用时传入的 `EDX` 错误/状态值。
- `0x34589B8` 调通用 helper `0x245ADDE`，helper 先查找既有 key，未命中时构造并挂入 node；插入包装返回 node `+0x28`，随后 `0x1EF48` 将源对象写入该槽。若槽原有非空对象，`0x1EF48` 通过其 vtable slot 0 传入值 1；该对象的具体 C++ 类型与完整 dispose 语义仍未知。全 `.text` 直接 `E8 rel32` 屏幕在有效 `.pdata` 函数范围内找到 helper 的两个调用点，其中 HttpServer 一侧通过 `0x34589B8` 到达；此屏幕不覆盖间接调用或所有控制流。
- 同一成功路径把 `HttpServer+0xE0/+0xE8` owner pair 与刚生成的 key 放入 wrapper capture，key 位于捕获对象 `+0x10`。§7.11 已用 RTTI 将 captured owner 静态归属为 `WeakWrapper<HttpServer@httpx>`；inline/heap invoker `0x3459220/0x3459426` 均进入 `0x345927C`。该 helper 从捕获对象 `+0x10` 取 key，weak-lock-shaped helper 成功后将 key 作为 `EDX`、调用方 DWORD 作为 `R8D` 送入共用 factory `0x3458A18`。
- factory 的 `R8D=0` 路径经 `0x3458B00` 构造连接，再用同一个 `0x34589B8` 包装把 `HttpConnection+0x18` key 插入 `HttpServer+0x60`；`R8D!=0` 路径对 `HttpServer+0x20` 按输入对象 `+0x10` key 精确查找并删除命中 node。结合本节写入点与 §7.12 删除点，已闭合这个 generic HTTP 路径中 `+0x5C` 生成 key → wrapper capture → `+0x20` 精确删除的局部关联；`+0x20` 与 `+0x60` 是不同 header，前者不应与连接表合并。
- 这仍不能证明 `+0x28` payload 在何处被调用、factory 的非零 gate 在真实运行中的业务意义、key 与 native response/request 的身份对应、实际 callback thread，或该表与全局 manager tables `0x73BC5C/0x73C2AA` 有关。它不是 QQ response-consumer 或 R4 现场证据，R2 owner/thread/manager ABI/teardown/quiescence 与 R3 stable identity 继续开放。

脚本先校验固定样本 hash，均保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\`：`r2-httpserver-tree-registration-consumer-detail-20261014.py/.txt`、`r2-httpserver-tree-insert-detail-20261014.py/.txt`、`r2-httpserver-map-payload-helper-raw-20261014.py/.txt`、`r2-httpserver-tree-direct-call-xrefs-20261014.py/.txt`，并与 `r2-alternative-parent-xrefs-20261014.py/.txt` 及 §7.12 的 branch/erase 导出交叉核对。无 QQ/SnowLuma 操作、native 函数调用、消息发送或 Cargo 测试。

### 7.17 ThreadTaskManager index-0 task 的动态类型（2026-10-14，静态）

继续核对冻结磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。使用 Capstone 5.0.6 按该映像 `.pdata` 有效范围解码构造/调用函数，并解析 MSVC x64 COL/RTTI；没有启动或读取 QQ 进程、调用 native 函数、发送消息或操作 SnowLuma。

- 与 §7.15 的 index-0 读取链相接：`D313D6` 把字节索引 0 交给 `ThreadTaskManagerImpl` vtable slot 1 `0x217D860`。`ThreadTaskManagerImpl+0x48` vector 项是 `TaskQueue` shared pair；slot 1 经 `0x31F83DC` 取出被索引的 pair，再调用 `0x63EC4` 从选中 `TaskQueue+0x38/+0x40` 复制嵌套 pair。因此 `D32138` 收到的是嵌套 `TaskRunner` pair，不是 vector 中的 `TaskQueue` pair。构造端 `0x217D58C → 0x217D6D8 → 0x217DEEA → 0x217DF64` 创建 `TaskQueue` shared pair；`0x31F8F5E` 在 `TaskQueue+0x38/+0x40` 安装每队列 pair，随后 manager build helper `0x31F8378` 收到 `TaskQueue` pair 及 `+0x48` vector 地址。安装 pair 的路径 `0x31F9016 → 0x31F9B36 → 0x31F9BBA` 创建 `shared_ptr_emplace<TaskRunner@TaskQueue>` 控制块；对象 ctor `0x31F9C3C → 0x31F878C` 将输入 pair 保存到对象 `+0x8/+0x10` 并写入 TaskRunner vptr。
- 固定映像 RTTI 交叉验证两层类型：控制块 vtable RVA `0x43B6248` 的 COL/TD 名称为 `__shared_ptr_emplace<TaskRunner@TaskQueue@internal@xpng>`；对象 vtable RVA `0x43B6088` 的 COL RVA `0x43B60B0` 指向 `.?AVTaskRunner@TaskQueue@internal@xpng@@`。因此可把 manager index-0 所取对象命名为 `xpng::internal::TaskQueue::TaskRunner`，不再把其直接称为未知类，也不误认为 `TaskQueue` 本体。
- `TaskRunner+0x8/+0x10` 的 pointee 由固定-hash 构造链闭合。`0x217D242` 在新建 manager 时先将 `ThreadTaskManagerImpl+0x10` 清零并安装 primary vptr；该 vtable RVA `0x4260448` 的 COL/RTTI 名为 `.?AVThreadTaskManagerImpl@internal@xpng@@`。Factory `0x217D35E` 构造指向 manager 的输出 pair/control block，再以 `(out_pair, manager+0x10, manager)` 调用 `0x1687C`；新对象的空字段路径经 `0x168CE` 将同一 `{manager, control}` pair 写到 `manager+0x10`。随后 `0x217D58C` 将该 pair 交给 `0x46448` clone，`0x31F878C` 把 object/control 两 qword 复制到 `TaskRunner+0x8/+0x10`。因此固定样本的 pair pointee 是 `ThreadTaskManagerImpl` 自身；这识别了 owner/context 对象，不识别其关联 task payload。
- `TaskRunner+0x8/+0x10` pair 的 owner 类型现已识别；`0x31F9228` 的业务效果、manager slot-2 mismatch 分支从自身 `+0x38/+0x40` 作出的 pair-shaped projection（字段边界见 §7.18）、slot-3 `manager+0x30` pointee/返回 pair、manager 其它项是否均被停止、child 最后 owner/析构时机以及 QQ dispatcher 全局排空仍 unknown。INFINITE wait 的返回值依旧未检查；R3 stable identity 与历史 AV 根因也未关闭，R4 仍不准入。上述均为固定-hash 离线静态结论，不证明运行时 owner 关系或 quiescence。

复核脚本与地址级导出在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\r2-nte-taskrunner-dynamic-type-20261014.py/.txt`；SHA-256 分别为 `DB2946623D6E62F2F8F123F9A8F2FBC88C8E5104709C7D9D0FF0793EF993EB54` / `70CA4E6B1A5D335A7A0A73C9FA16D380E82F13B97A4456D33F5BE8ACA27FC3E4`。脚本读取前校验样本 hash，导出有效 `.pdata` 函数范围与两组 RTTI；任务构造调用链由同目录 `r2-nte-manager-task-init-20261014.py/.txt` 与 `r2-nte-taskqueue-item-transform-20261014.py/.txt` 补充；`0x63EC4` 的有界 helper 导出由 `r2-nte-bounded-helper-targets-20261014.py/.txt` 交叉复核，SHA-256 `2A8A96FA0897D454E4D16BEB8ED80ED2972E4B05B57D936F94A9EFC7E92DD90D` / `F99FAAF2F77E42DC76D9A8F4E9AC655B8BC98FAF0A1A48DD8B4C0860C2FB32CC`。该静态类型识别只闭合 index-0 wrapper 的类名，不证明队列语义、callback 执行或停机安全。
本次 pair-owner 追查的固定-hash Capstone 5.0.6 导出保存在同一 `_reference` 目录：`r2-nte-manager-inner-pair-followup-20261014.py/.txt` SHA-256 `22E344C4AE6BC4049D93575C86ACC37CE8EC05929B8D5FD70CB800AE5DB641BF` / `AF0D57B43EBBB398EA97A575437C2536BD6A08966650A442E8CD9A3FB772C4D5`；`r2-nte-weak-pair-helper-followup-20261014.py/.txt` `A89494EFF657CFF366B2AA472449F7C816561380D0C8B6A0725202088D70DCE2` / `AA2B75BCC7E72DEE005A65B82137593394B1BD15ECFCEC1C4441C39E4EBC182D`；`r2-nte-taskrunner-vtable-methods-20261014.py/.txt` `AE1CDCDD96D40429FF37044AFE83128A28A9394148F80A247E9920A49067666D` / `009DA7443DD0A4534F99932524AE657CD396F1C49F82BBFDA9CE5070F8ED5300`。每个脚本先验证同一 `wrapper.node` SHA-256，并限于该映像 `.pdata` 函数、pair-copy helper 与 RTTI/vtable 数据。此前 `TaskRunner+0x58` 的归属描述根据本次函数级复核更正；这不改变未知任务语义和现场准入状态。

### 7.18 ThreadTaskManager TLS pair 来源与字段边界更正（2026-10-14，固定样本静态）

继续核验同一冻结磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Capstone 5.0.6 按 `.pdata` 边界解码 `0x217D242/0x217D4FA/0x217D8CC/0x217DA24/0x217DA96` 与 helper `0x63EC4/0x1DB2/0xD32E5A/0xD32EDA`；没有启动或读取 QQ、调用原生函数、发送消息或操作 SnowLuma。

- `ThreadTaskManagerImpl` ctor `0x217D242` 以 16-byte 零写初始化 `this+0x30/+0x38`，另清零 `this+0x40`；dtor `0x217D4FA` 分别对 `this+0x40` 调 `0x217D4F2`（空值替换并释放旧 holder）及对 `this+0x30` 调 `0x1DB2`（释放 pair）。结合 `0x217D556` 清理从 `+0x48` 开始的 vector，字段边界应读作 `+0x30/+0x38` pair、独立 `+0x40` holder pointer、`+0x48` task vector；不由这些字段操作推出 runtime pointee 类型。
- manager slot 2 `0x217D8CC` 将 `this` 写入一个 TLS context 槽后，加载 `[this+0x30]` 作为 pointee，再调用 `0x63EC4` 复制该 pointee 自身 `+0x38/+0x40` 的 16-byte pair。`0x217DA24` 分配 `0x10` 字节；`0xD32E5A` 将该 pair 复制到分配块、对 source control block `+8` 增加强引用、调用 `0xD32DAD → 0x104C1C8` 安装到另一 TLS 槽，并释放临时 source pair。返回的 holder pointer 随后经 `0x217DEB0` 写入 manager `+0x40`；替换旧值时 `0x217DEC8 → 0xD32EDA` 释放其中 pair，再经 `0xB750029` 释放 16-byte block。由此更正 thread contract §3 的早期简写：安装目标不是已证的 manager `+0x30` pair 首 qword，而是从该字段 pointee 的 `+0x38/+0x40` 复制出的 pair holder；其 pointee RTTI 仍未知。
- manager slot 3 `0x217DA96` 同样先加载 `[this+0x30]`，再由 `0x63EC4` 投影该 pointee 的 `+0x38/+0x40` pair。它与 manager mismatch 路径不是同一地址来源。
- `0x31F88EA` 的 TID 不相等路径则把 manager 本身交给 `0x63EC4`，机械读取 manager `+0x38/+0x40` 并把复制结果首 qword 传给 `D32138`。但前述 ctor/dtor/slot-2 写入显示 `manager+0x38` 属于 `+0x30` pair，而 `manager+0x40` 是指向 heap pair holder 的独立 pointer；因此这里不是已识别的 `ThreadTaskManagerImpl` 自有 `{object, control}` pair。保留指令级 projection，不认定其 nested pointee、pair 有效性或故障原因，也不把 slot-3 对 manager `+0x30` pointee 的合法投影混为一谈。
- 本地固定-hash 脚本/地址级导出：`E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\r2-nte-manager-field-owner-clarification-20261014.py/.txt`，SHA-256 `ED299ABF97A4351292ED7370A941421D8F794B30D59F3E8147CEA17EA2ABE2BA` / `134996C57B11C70B644F97CC5AC827688AA5CC6A22E2D5307304B27D9CC10C27`。脚本先验证冻结样本 hash；pair helper 的机器行为与对象语义分开记录。manager `+0x30` pointee 来源、TLS slot0 实际目标类/方法、mismatch projection 的运行时合法性、thread stop/drain/quiescence、历史 AV 根因和 R3 identity 仍 unknown。本文更正不提升 R2/R3 gate，R4 仍不准入。

### 7.19 ThreadTaskManager `+0x30` 的条件写入与源 pair（2026-10-14，固定样本静态）

继续核验固定磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Capstone 5.0.6 按 `.pdata` 边界复核 `0x217D58C`、`0x217D6D8`、`0x217DEEA`、`0x217DF64`、`0x106E6` 与构造路径；没有启动或读取 QQ、调用 native、发送消息或操作 SnowLuma。

- §7.18 已确认 `ThreadTaskManagerImpl+0x30/+0x38` 的字段边界，但当时其 writer 未知。本轮在 manager 初始化方法 `0x217D58C` 找到条件 writer：`r15b` 从 0 递增，循环条件为 `r15b < byte[this+0x2C]`；每轮先把生成的 task pair 送入 `this+0x48` vector helper `0x31F8378`，随后只有 `r15b == byte[this+0x2D]` 才调用 `0x106E6(&this+0x30, &local_pair)`。这两处 byte 仅按机器码作用称为 loop bound 与 selector，不为其业务配置命名。
- `0x106E6` 对 source control word 非空时先增加强引用，把 source 的两个 qword 复制到 destination `+0x30/+0x38`，再以 `0x1DB2` 释放被替换的旧 pair。因而已闭合一条真实的 conditional pair assignment；它不表明每条初始化路径都满足 selector。若循环不进入或没有相等的 byte index，本方法不会写该 pair；只有在不存在其它 writer 的路径上，构造函数零值才会保留。本节没有对整个映像的其它潜在写者做否定扫描。`0x217D6D8 → 0x217DEEA → 0x217DF64/0x1676E` 生成 source local pair；helper `0x2509E` 的产物类型、source object RTTI/用途及 TLS/dispatcher 身份仍未识别。
- 现有 slot2 `0x217D8CC` 与 slot3 `0x217DA96` 会从 `[this+0x30]` 取 pointee，再用 `0x63EC4` 读取该 pointee `+0x38/+0x40`。`0x63EC4` 对传入 pointee没有空指针检查。本轮已为两个精确构造点复原 loop 配置并证明其 selector 命中，见 §7.20；但这只覆盖 30 个 `0xD31686` 直接调用点中的两条代码路径，未将任一静态路径绑定到 T1–T5 的被观测 manager，也未封闭 slot2/slot3 的全局调用者顺序。`0x31F88EA` mismatch route 对 manager 本体作出的跨字段 projection 仍与此合法 field writer 分开，不能据此命名其 nested target或归因历史 AV。
- 固定-hash 复核脚本/输出位于本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`：`r2-manager-writer-source-context-20261014.py/.txt` SHA-256 `0F47D5E0E51DBA9432C4AF2AA4CF37EBFB25F48B3929112D84BDB36C9F989BC7` / `C4D82FE82F24900CE03EC003E024786694240FBF41C2D10A18C723614346877C`。脚本先校验冻结样本 hash，导出限于所列 `.pdata` 函数。此项不关闭 manager owner/thread/teardown、全局 callback drain/quiescence、R3 stable identity 或历史 AV；R4 仍不准入，产品 NOT READY。

### 7.20 ThreadTaskManager 配置字节、TaskQueue RTTI 与两个命中路径（2026-10-14，固定样本静态）

继续核验磁盘 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Capstone 5.0.7 对 `.pdata` 函数作有界解码；`0xD32508`/`0xD32512` 是不带 `.pdata` 的短 helper，报告只解码其相邻有界窗口。没有启动/读取 QQ、调用 native、发送消息或操作 SnowLuma。

- `0xD32508` 将 `DL/R8B` 写到目标相邻两字节，`0xD32512` 复制一个 WORD；`0xD324EC` 用 `{1,0}` 调前者；`0x4E6E5A` 先清零 config 首 DWORD、再调用 `0xD324EC`。所以输入给 `0xD31686` 的 6-byte config 布局可按实际写入追踪到 `ThreadTaskManagerImpl+0x28..+0x2D`，其中 `+0x2C` 是 loop bound byte、`+0x2D` 是 selector byte。
- 固定构造路径 `0x9C0C68 → 0xD31686` 在 `0x9C0E42` 清零 config 首 DWORD；`0x9C12A2` 读取其首 DWORD 作为值传递给下游，函数体没有把源地址再传递或向源写入；随后 `0x4E6E5A` 重置 config，`0xD32508(...,3,1)` 与 `0xD32512` 将尾部两字节改为 `{3,1}`；`0xD3251C` 将首 DWORD 与尾部 WORD 复制给 `0xD31686`。`0xD31686 → 0x217D16C → 0x217D242` 保持该 6-byte 布局并复制到 manager `+0x28`。该路径的 `+0x2C=3, +0x2D=1`，故 `0x217D58C` 遍历 index 0、1、2，并在 index 1 执行 pair writer。
- 第二个直接调用点 `0xB81758 → 0xD31686` 先清零 8-byte 栈槽，再由 `0x4E6E5A` 设置尾部 `{1,0}`，传入 config 的可读 6 bytes 为 `{0,0,0,0,1,0}`。对应 `+0x2C=1, +0x2D=0`，初始化循环进入一次并在 index 0 写入 pair。两个 callsite 均在 `0xD31686` 中先把 manager shared holder 安装到外层 `+0x110`，再同步调用 `0x217D58C`；之后才调用线程创建 helper `0xD33418`。这是具体 setup 顺序，不证明所有方法或调用者都遵循同一时序。
- RTTI/xref 复核确认 `0x217DEEA → 0x217DF64` 构造的控制块类型为 `std::__shared_ptr_emplace<TaskQueue, allocator<TaskQueue>>`，pair 的 object 指向控制块内嵌 `+0x18` 对象；source 因此可命名为 `TaskQueue` shared pair。两个已追踪输入路径会命中 writer，缩小了此类实例的空 pair 风险；全映像 30 个 `0xD31686` 直接调用点中其余路径、异常/失败边界，以及该 manager 与 T1–T5 动态 dispatcher 的对应关系仍未闭合。不能推断 slot2/slot3 所有调用都晚于该初始化，也不能把此结果归因为历史 AV。

固定-hash 复核文件在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`：`r2-manager-config-producer-20261014.py/.txt` SHA-256 `FF9E4C0D8EEACCB05A6E5079E00592495DE53426BC8E5A5A782D519FEFD04FBF` / `0056F41CCFC68695173E6C051E1B08CB7EB80EF7AC7B993DC0FBE2649B66DBB1`；`r2-manager-init-dispatch-rtti-20261014.py/.txt` `AD02A313D5336241E0012DCAE643E0CF6C738A0CDAA7D422C16405167B8F908A` / `DC964B06DDF2237C9CD6207ABC3DA95D021B56923DCD78A2A3301FCEA78F1930`。脚本对样本 hash 作校验；报告不提升 R2/R3 gate，R4 仍不准入，产品 NOT READY。

### 7.36 `MSFService@nt` vtable slots 3–4 route through the send dispatcher (2026-10-14, fixed-sample static)

继续只读固定 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Capstone 校验 rel32 `E8` 指令边界并以 `.pdata` 确定函数范围；RTTI 检查要求 MSVC x64 COL signature/self-RVA 一致。

- 固定样本 `.rdata` 中的 vtable RVA `0x3F6E1C8` 紧邻 COL `0x3F6E600`；COL offset 为 `+0x10`，TypeDescriptor 为 `.?AVMSFService@nt@@`。该 vtable slot 3=`0x7329F6`、slot 4=`0x7333AC`。两函数都含经 `.pdata` 边界解码验证的直接调用 `0x732A80`：分别位于 `0x732A3E` 与 `0x7333E6`。slot 3/4 方法从其 `+0x10` secondary subobject `this` 调整回完整对象后进入 dispatcher；slot 4 调整指令为 `this-0x10`。这是固定映像中的 MSF service submission surface，不证明某现场运行实例执行过该路径。
- 该 caller edge 将已知 `0x732A80` body/callback-manager transfer 路径收敛到 `MSFService@nt` 的两个具体虚槽。它没有识别 `0x73BD6C` 的实际 `param_1+0x30` callback target、callback owner/consumer TID、所有间接 manager caller 或 response identity；也不证明 stop/cancel/drain/quiescence、历史 AV 根因或 R3 stable identity。
- 可复核本机产物位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\response-consumer-followup-20261014\`：`r2-response-consumer-direct-calls-20261014.py/.txt` SHA-256 `864BACD0E75B5DF8ACC0DD447B1AC5E434D782EFABDA392AC4DA67461AE6DA41` / `9DB4C279B77C045E476B02D7A7CF2B1C8AD24F064CF52AD2DE12A8565298CD1C`；`r2-response-consumer-msfservice-vtable-verified-20261014.py/.txt` SHA-256 `F5B985411E1F359712BAF0363CC87E2343C1082F941ED07DE5CA85627B066E23` / `0155E446D470A7456602C6C67D8D56BFB4992B4CE6D180A8DED82481DB1E449B`。脚本均拒绝非预期模块 hash；直接调用筛查只覆盖固定映像可识别 `.text` rel32 `E8`，RTTI 脚本只验证这两处方法地址与指定 MSFService vtable slot。所有间接调用及映像外路径仍在范围外。
- 本增量仅定位静态提交入口；R2 consumer ABI/thread、last-owner/teardown/quiescence 与旧 AV、R3 identity 仍 open。R1–R3 门未满足，不准入 R4；产品 NOT READY。未访问 QQ/SnowLuma、未调用 native、未发送消息或运行 Cargo tests。

### 7.37 `MSFService` callback wrapper reaches an unresolved transport vtable slot (2026-10-14, fixed-sample static)

继续只读固定 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。脚本按 `.pdata` 函数范围解码目标窗口，并逐个验证这条回路中的 9 个直接 `E8` 调用边界。

- RTTI 已将 `0x7333AC` 归到 `MSFService@nt` vtable slot 4（§7.36）。它调整 `this-0x10`，在第五个参数构造带两函数指针的临时值：`0x2D338` 与 `0x2D36A`，再以 `r9=0` 调用 `0x732A80`。两函数分别对传入上下文的 `+0x3C8` 调 `1DB2`、对 `+0x420` 调 `75B8E0`；这里只记录机器码行为，不把它们命名成业务回调。
- `0x732A80` 将额外描述符经 `31C5F8` 移入局部捕获，调用 `0x73BB94` 转移 manager 值，再调用 `0x73BBF8` 构造 manager-backed callback wrapper。`0x73BBF8` 在 `4D3588` 返回非空时选 inline wrapper `0x73BC5C/0x73BD08`；为空时分配 `0x40` 并选 heap wrapper `0x73C2AA/0x73C34C`。两条 invoke wrapper 都会到 `0x73BD6C`。
- 提交点先检查 `MSFService+0x60`，非空时读取其中对象的 vtable slot 7（vtable `+0x38`）并间接调用。此调用同时传入请求、捕获上下文、manager callback，以及两个栈上传入值。`MSFService` 构造函数 `0x72DF56` 将包含 `+0x60` 的字段区清零；本轮没有找到运行期写入者或该对象的动态 RTTI 类型。故已知的是接口槽与 callback wrapper 的静态传递，未知仍包括实际 transport 对象/目标方法。
- `0x73BD6C` 最终经 `param+0x30` 的函数指针调用 consumer，参数形状为 `(param+0x18, 0x10, errCode, errString, msgUid)`；`msgUid` 来自响应对象 `+0x38`。这补充了结果 invoker 的静态 ABI，仍不能确定现场 consumer 身份、线程、线程切换位置或 callback queue teardown。

本机固定样本产物在 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\response-consumer-followup-20261014\`：`r2-msfservice-callback-transfer-20261014.py/.txt` SHA-256 `72448F186E92FC2935FC5CF0B56135189A31D0C5AE38AED00FBC54BEB6C3E52B` / `BA619295843FA0886EAF1ABF60C4053B74FFBEECBC1093B783CEAE559350C5E2`。脚本拒绝非目标模块 hash；9 条直接调用均经 Capstone 指令边界与 `.pdata` 归属复核。它不覆盖间接调用目标的现场解析、间接 manager callers、owner last-release、stop/cancel/drain/quiescence 或旧 AV 根因。R2/R3 未闭合，R4 不准入，feat-005/006 blocked，产品 NOT READY；未访问 QQ/SnowLuma、未调用 native、未发送消息或运行 Cargo tests。

### 7.38 `MSFService+0x60` 固定样本动态类型链至 `MSFMessengerImpl`（2026-10-15 静态）

继续只读同一 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。本节把 §7.37 未知的 `+0x60` writer/动态类收窄到固定映像 RTTI/vtable；不把静态对象构造路径等同现场实例或线程验收。

- RTTI 识别 `MSFService@nt` primary vftable `0x3F6DED8` 的 slot 16 为 `0x72EC24`。方法以 `rsi=this`：`0x72F4B0` 取 `this+0x50` 地址，`0x72F4C1` 通过 16-byte move helper `0x2EC8` 安装其 shared pair；稍后在 `0x72F56D` 取该 pair pointee，并于 `0x72F59A` 调其 vtable `+0x48`（slot 9）。该结果 pair 再于 `0x72F5AB` move 到 `this+0x60`。故两字段均有静态 writer，且 `+0x60` 值来自 `+0x50` 对象的 slot 9 返回路径。
- `0xB78200` 为 `+0x50` 创建 0x310-byte 对象并调用 `0xB78380`。对象构造 vptr 的 COL/RTTI 为 `MSFSessionImpl@msf`，primary slot 9 的地址为 `0xB7C6C0`。该方法分配 0x70-byte 对象并调用 `0x1B4DFF8` 构造；此处 vptr 的 COL/RTTI 为 `MSFMessengerImpl@msf`。随后 `0xB7C89C` 建 shared ownership pair 回给 `MSFService` 的 `+0x60`。
- `MSFMessengerImpl@msf` primary vftable `0x41403B8` 的 slot 7（vtable `+0x38`）为 `0x1B4E4EC`。这与 §7.37 `0x732A80` 从非空 `MSFService+0x60` 加载 vtable slot 7 的间接 call 汇合。`0x1B4E4EC` 继续到 `0xB7CE8A`；其路径读取 `0xB816E8` 返回对象状态，并最终在 `0xD32138` 对所得对象间接调用 vtable slot 0。
- 由此闭合固定样本中的 service submission → `MSFSessionImpl` slot 9 → `MSFMessengerImpl` slot 7 → callback/dispatch continuation 的静态类型边。仍未知：`0xB816E8`/`0xD32138` 最终对象的动态 RTTI 与调用目标、consumer TID/线程切换、manager 全部 owner 的 last release、停止/取消/队列 drain/quiescence，以及历史 AV 根因。R2/R3 完成门未满足，R4 不准入、feat-005/006 blocked、产品 NOT READY。

hash-gated 脚本/报告保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\response-consumer-followup-20261014\`：`r2-msfservice-messenger-vtable-chain-20261015.py/.txt` SHA-256 `5FB98AB88FCDA6C6BA847E91CA5D63064E9D4716229552D69B250484680EE505` / `8FF3B7F636B25066AA9B8A8F4B7C0C8FC5CAEA877F73500241A9318E99BD7BAB`。脚本校验 PE `.pdata`、COL/RTTI、三张 vtable 的指定 slot 与关键 call 指令；不执行 QQ/native。

## 8. 对实现的硬约束

1. Rust FFI 声明只能引用本文 `[verified]` 的几何与函数地址；`[inferred]` 项进入实现前必须先闭合。
2. 一切从 QQ 返回的 pair 按 opaque 处理：只在 FFI 边界用两个 pointer-width words 保存 `{obj,ctrl}` 并原样传给 `1DB2`；禁止解引用/修改控制块内部计数或把其布局当作稳定 C++ 类型。
3. 释放必须走 QQ 侧函数；禁止跨 allocator。
4. `732A80` 同步深复制 `[begin,end)` body；本侧 body input 可在函数返回后经配对 `B75029` 释放。callback reason/body 仍只在 callback 调用期内有效，进入 Rust 前完成拥有复制。
5. 已取 pair 在尚未 submit 且 attach 失败时可走 `1DB2`；尝试过任何 native submit 后，不在 adapter Drop 中释放，直到 callback/dispatcher quiescence 有证据。
6. 任何向 QQ 注册 Caligo callback 的路线都必须先 pin bridge image 到进程终止；pin 失败时在 native submit 前拒绝。没有 callback quiescence 证据时，不恢复热卸载。
7. 本文件全部结论基于静态固定样本与所列离线实现；不构成任何现场准入门（G1–G4）的通过证据。

### 7.12 `HttpServer` 替代 factory 的 R8D 分支与候选查找（2026-10-14，静态）

本节复核 §7.11 的两个分支，仍只读取冻结磁盘样本 `wrapper.node`，SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。这里把机器级分支值与业务语义分开：`0x3458A18` 保存 `EDX`，实际以 `R8D` 作零/非零分支判断；目前没有证据把该 DWORD 命名为业务状态或 callback 结果。

- `0x345927C` 将 `dword [rdx]` 传到 factory 的 `R8D`，并将其输入对象 `dword [rdi+0x10]` 传到 `EDX`。直接 fallback caller `0x3458736` 在 `0x3458899` 传 `EDX=0`、`R8D=0`，同时传入从 `HttpServer+0x38` 移出的 pair。零分支随后递增 `DWORD [HttpServer+0x58]` 并进入 `0x3458B00`；这里只记录写入与路由，不给 `+0x58` 指派 generation/业务 ID 语义。
- `HttpServer` ctor `0x3457D10` 对 `this+0x20` 与 `this+0x60` 调初始化 helper `0x3BA6→0x3CEA`；`0x3CEA` 将 header 首 qword 指向 `base+8` 并清零后续 16 字节。dtor `0x345808E` 对这两个字段调用 `0x3FBAC6`，递归遍历 node `+0/+8` links，析构 node `+0x28` 的 shared-pair，再经 `0x4C5C8` 将 factor 1 与 node 指针传入 sized helper。`0x6AB34/0x6AB9C` 显示 `HttpServer+0x20` 是 tree-shaped header、`+0x28` 是 sentinel；`0x6AB9C` 按 node `+0x20` DWORD 做有序下界搜索，`0x6AB34` 再检查查询键是否等于候选键，不等时返回 sentinel。键源追到 `0x345927C`：factory 收到的查找键来自其输入对象 `+0x10` DWORD；命中的 node pointer 随后作为 `R8` 传入 `0x5D763A`。故这里静态证明的是按该 DWORD 精确命中后删除树节点，不只是一条可能 erase 的结构形状。键的产品意义仍未绑定，也没有 RTTI 证明具体 C++ 容器模板。
- 命中时，`0x5D763A` 以搜索返回的 node pointer 调 `0x1086E`：该 helper 取得 successor、在删除根节点时更新 root、递减 tree size 并调 `0x108CC` 执行链接/颜色重平衡。随后 `0x5D763A` 析构 `node+0x28` 的 pair，并将 node 指针与因子 1 交给 `0x4C5C8`；后者把 `0x30` 大小交给 `0x1FD0`。这证明精确键命中后的 RB-tree erase 和节点 payload 清理；pair pointee 类型、key 的业务含义以及这一操作在上层 callback 流程中的目的仍 unknown。factory 丢弃该 helper 的返回值，不能把它称为 QQ 响应 handler。
- §7.11 的“state 0/非零 state”仅描述机器分支。精确表述是 factory 的 `R8D` 零/非零 gate；其来源只追到 invoker 经 `0x345927C` 的寄存器传递，调用时业务意义仍 unknown。查找键的机器来源是同一输入对象 `+0x10` 的 DWORD，RB-tree 按此键精确选择节点；该键是否为 callback/request/business ID 未知。替代路径仍是通用 `HttpServer@httpx`；不证明 QQ response consumer、回调线程、global manager ABI、teardown 或 quiescence。

可复现脚本及地址级导出仍在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\httpconnection-field-owner-20261014\`：`r2-httpserver-weak-capture-and-state-route-20261014.py/.txt`、`r2-httpserver-branch-helper-disasm-20261014.py/.txt`、`r2-httpserver-branch-helper-raw-20261014.py/.txt`、`r2-httpserver-owner-container-disasm-20261014.py/.txt`、`r2-httpserver-container-lifecycle-20261014.py/.txt`、`r2-httpserver-container-helper-raw-20261014.py/.txt`、`r2-httpserver-tree-update-disasm-20261014.py/.txt`、`r2-httpserver-sized-node-helper-raw-20261014.py/.txt`、`r2-httpserver-tree-erase-helper-disasm-20261014.py/.txt`。两个 branch-helper 脚本读取前校验 SHA-256；`0x6AB9C` 报告明确标为非 `.pdata` 函数起点的有界指令解码。

### 7.14 `NTEngine` parent field release 与 slot-1 边界（2026-10-14，静态）

继续核对冻结磁盘 `wrapper.node`，SHA-256 为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。只读 `.pdata`、RTTI、反汇编；没有启动/读取 QQ 进程、调用原生函数或操作 SnowLuma。

- RTTI 将主 vtable `0x3FAD868` 归属 `.?AVNTEngine@nt@@`；slot 0 deleting-destructor thunk `0x9C2C6C` 调析构主体 `0x9C15D6`。构造路径 `0x9C0C68` 初始化 parent 字段对 `+0xE0/+0xE8` 与 `+0xF8/+0x100`；从这些字段取得的子对象指针会进入 `0xD31686`。该 helper 在其输入子对象基址 `+0x110` 安装 ThreadTaskManager holder 并调用启动方法。
- NTEngine 析构主体先对 `this+0x150` 执行虚槽 `+0x10`，随后调用 `0x9C1700`，再分别处理 parent 的 `+0x150/+0x140/+0x130/+0x120/+0x110/+0x100/+0xF8/+0xE8/+0xE0` 字段。机器码显示 NTEngine 自身 `+0x110` 走 `1DB2` pair release；`+0xE0/+0xE8` 与 `+0xF8/+0x100` 通过既有 owner/pair helper 拆分清理。由此只证明 parent 析构路径会释放其持有的字段引用。
- 偏移必须按基址区分：`NTEngine this+0x110` 是 parent 自身字段；`D31686(input_child)` 写入的是输入子对象的 `child+0x110`。当前静态材料未证明后者的最后一个 owner 何时释放、child destructor 是否运行、holder 何时销毁，也未证明 dispatcher 停止或排空。析构中对 `this+0x150` 的虚调用及 `0x9C1700` 的完整业务语义/时序合同仍未解析。
- NTEngine vtable slot 1 `0x9C2458` 将 `[this+0x110]` 作为第一个参数传给 `0x173D042`，但该有界 leaf 不读取 RCX，只在第二参数所指输出位置写入 `{self,self,0}` 形状后返回；这不是 teardown 证据，类型与语义仍 unknown。
- 构造 helper `0x9C12A2` 在 `0x9C1309` 对绝对地址 `0x180D3111E2` 发起 direct call。该 VA 在 `wrapper.node` 与精确版本磁盘 `QQNT.dll` 的 preferred-image 范围之外；把假设 load base `0x180D000000` 代入会得到 QQNT RVA `0x3111E2`，但没有当次模块基址/重定位证据，且该 RVA 落在函数 `0x3111A0–0x3113F3` 的分支中段。因此不将目标归属给 QQNT，也不据此命名子对象或归纳其 owner 合同。

可复核导出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\`：基础固定-hash 脚本/输出 `r2-nte-engine-child-owner-dtor-20261014.py/.txt`，补充 parent release 记录 `r2-nte-engine-outer-release-20261014.py/.txt`（补充脚本/输出 SHA-256 分别为 `74DFE345F4CA226D4303688DFCBA1ADB80375CE5361E1084CA49BDFB0647237A` 与 `3C50FC314BB039D3731A45C9392DC408B9AC1A9D61D0AB6CEFE903CDBBBC98D6`）。补充脚本校验基础脚本 hash；基础脚本再校验 `wrapper.node` 与磁盘 `QQNT.dll` hash。固定映像只读证据只收窄 NTEngine parent 字段释放路径，不闭合 child last-owner teardown、全局 manager ABI、callback/consumer thread、QQ stop/cancel/drain/quiescence、历史 AV 或 R3 stable identity；R4 准入及 NOT READY 状态不变。

### 7.15 child `run_loop_thread.cc` stop 与 wait-handle 路径（2026-10-14，静态）

复核同一冻结磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。使用 Ghidra 12.1.4 `-noanalysis` 对精确 `.pdata` 范围定向反汇编/反编译，并用 Capstone 5.0.6 复核指令边界；没有启动/读取 QQ 进程、调用原生函数、发送消息或操作 SnowLuma。

- shared-control release path `0x1B643C0` 取出 control `+0x8` 的 child 指针、先清空该槽，再调用 child dtor `0xD31356`，最后释放 child；`0xD31356` 先直接调用 `0xD313D6`，随后才对 `child+0x110` 调 `1DB2`。因此，**当且仅当该控制块释放路径实际到达**，child dtor 会先执行下述 pre-cleanup，再释放其 manager holder。何时到达该释放路径、是否为最后 owner，仍未证明。
- `0xD313D6` 的日志源字符串指向 `..\foundation\xplatform-ng\xpng\task\run_loop_thread.cc`，固定样本还含 `Stop Thread Begin`、`Stop Thread Waiting Join`、`Stop Thread Join Done` 与 `Stop Thread thread is null, return`。函数先检查 `child+0x29` guard 与 `child+0x110`，设置 guard 后经 `0xD31AC0` 从该 holder 取 pair。
- `ThreadTaskManagerImpl` 有效 RTTI vtable RVA `0x4260448` 的 slot 1 指向 `0x217D860`。该槽从 `ThreadTaskManagerImpl+0x48` pointer-backed task vector 依 byte index 复制一个 shared pair。`0xD313D6` 把 `R8` 设为 RVA `0x3FED904` 的地址；固定样本该地址首字节为 `0`，`0xD31AC0` 保留此寄存器，slot 1 读取 `[R8]`，故该静态路径取 vector index 0。随后 `0xD32138` 对所取对象调用 vtable slot 0，并传入当前 child 与一个 0x10 字节上下文。任务构造走 `0x217D58C → 0x217D6D8 → 0x217DEEA → 0x217DF64` 一带的类型擦除/helper 路径；该 entry 的动态类与 slot 0 的代码语义仍 unknown，不能命名为 manager 全局 stop 或 callback drain。
- 若 `child+0x10` 非空，`0xD313D6` 调 `0xD3355C`；Ghidra 反编译为 `WaitForSingleObject(*handle, 0xFFFFFFFF)` 后 `CloseHandle(*handle)`。wait 返回值没有被检查；调用者随后无条件写 `child+0x10=0` 并记录 Join Done。故静态可确认的是 stop 日志/虚调用路径和无超时 wait + close 的代码形状；日志本身不证明 wait 成功或线程已退出。空句柄分支记录 thread-is-null 并继续清理。
- 这比 §7.14 多闭合一个 child 局部 stop/wait 路径，不闭合 child last-owner / 析构是否触发、`+0x10` handle 的创建与对象类型、index-0 task 的动态类型、ThreadTaskManager 其它 entry 的停止、QQ callback queue drain 或 global dispatcher quiescence。不能将该证据映射成 QQ response consumer 已安全停止，也不能据此解释历史 AV 或放行 R4。

固定 hash 可复核脚本/输出位于本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\`：`r2-nte-thread-stop-path-20261014.py/.txt`（脚本/输出 SHA-256 `2D62A6D711792A6277B9E855B78B408E90AB772E600518E5DB496B4E8A5FBE4C` / `CB8531753CDC53A97F6CAA149DB07EECF98F19B0E5D53E6C1365AC797F7ED069`），以及 `ghidra-script\DumpNteCleanup.java`、`DumpNteCleanup2.java`、`DumpNteManagerBuild.java`、`DumpNteTaskFactory.java`、`DumpNteTaskFactoryImpl.java` 和 `ghidra-cleanup-20261014\` 定向输出。Ghidra scripts 在读取前核对目标 SHA-256；`.pdata` 与函数导出是有限定界分析，不是全映像语义恢复。
### 7.16 child+0x10 HANDLE 的 `CreateThread` 写入来源（2026-10-14，静态）

继续核对同一冻结磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。按有效 `.pdata` 边界导出 `D31686` / `D33418`，将 PE IAT 映射为 `KERNEL32.dll!CreateThread`，并用直接 callsite 与 bounded callback-target 导出交叉检查；不读取或操作运行中的 QQ。

- `D31686` 在调用 `D33418` 前于 `D31779` 计算 `rax = child + 0x10`，将其写入 Win64 第五参数槽 `[rsp+0x20]`；`D31782` 的 RIP-relative 地址是 callback thunk `D31958`，`R9` 保持 child 指针。直接调用位于 `D31791`。
- `D33418` 把 callback 与 child 放入线程启动 context，以固定 trampoline `D334B7` 调 `KERNEL32.dll!CreateThread`。非空返回值保存在 `rsi`；成功分支从本函数入口对应的第五参数槽取出 out-pointer，并在 `D3348F` 写入 `*out = handle`。因此在 `D31686` 这条固定样本路径上，`CreateThread` 成功返回的 HANDLE 写入 `child+0x10`。CreateThread 返回空时走错误分支并返回失败，不在该分支写入句柄。
- 线程 callback 指针 `D31958` 是跳转到 `D31B7E` 的 thunk；有界解码显示 `D31B7E` 接收同一 child、写入 `child+0x60` 当前线程 ID，并通过 `child+0x110` manager holder 作虚调用。它不能识别 manager 的完整任务语义，也没有证明 response callback queue 已排空。
- 与 §7.15 对照，析构前 `D313D6` 读取的正是该 `child+0x10`：非空时传入 `D3355C`，对句柄执行 `WaitForSingleObject(INFINITE)` 和 `CloseHandle`。等待结果仍未被检查，随后清槽和 `Join Done` 日志不证明等待成功或线程已退出。

这闭合的是固定样本中 child 局部线程 HANDLE 的成功创建写入点以及对应的 wait/close 读取点；不证明 child 的最后 owner 或析构时机、等待成功、ThreadTaskManager 所有 entry 均停止、global QQ callback queue drain/quiescence、R3 稳定身份、历史 AV 根因或真实响应交付。R1–R3 门仍未闭合，不能开始 R4。

本机可复核文件位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\`：`r2-nte-thread-create-path-20261014.py/.txt` SHA-256 `291651009C1DC4F324D3B5A29696C6569DAD73D611D9DF9085552DFEFE0B6610` / `92EEC121172EF753FC7DB7B3760598391A8B952EA9A0D6CB82CE39ADB329540F`；`r2-nte-thread-create-xrefs-20261014.py/.txt` `0D064B7FD07D80B07EBB1B4A0B12103382C4CF0CF4F81359742DDB8CC77CADBE` / `DC151632F85148DEE445CFCE06EE5FAE23579B560390CC907A46E9FB33AE88CF`；`r2-nte-thread-callback-target-20261014.py/.txt` `D035913AAB71B8F7EFBC284F14488C2F66FCA5AF907C78564FAF004417941AA1` / `DACF2D290604E84A77CBA72F0FB9EEDD23434E0F8E7603A6D47FE83982E2A03F`；PE import/callsite inventory `r2-nte-handle-writer-scan-20261014.py/.txt` `58D147AF23E96AB389C67088448D2B26886ED6D36BB50D0F1D561C97228D973C` / `2567B63F946C700DC36AD465639ACDE95843AFB09D8645FC8AE278C1C58B9F96`。这些脚本均对样本 hash 作校验；扫描仅覆盖映像导入与 `.pdata` 有效函数边界，不能据缺失 callsite 推断不存在路径。

### 7.21 全部 30 个 `D31686` 直接调用点的 manager 配置来源（2026-10-14，固定样本静态）

对同一冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）用 Capstone 5.0.7 按有效 `.pdata` 范围枚举并校验 rel32 `E8` 直接调用，找到 30 个 `0xD31686` callsite。逐点反汇编其配置指针来源后，30 个 callsite 归入以下三个静态模板：

- 28 个通过 `0x4E6E5A` 构造六字节配置；该 helper 清零首 DWORD，再在 `+4` 调用 `0xD324EC`，后者写入字节 `{1,0}`。调用点为 `0x004E69EB`、`0x0072001C`、`0x007200A3`、`0x007268C0`、`0x009C14BD`、`0x009C4951`、`0x009C5529`、`0x009DA908`、`0x00AFD53C`、`0x00AFF5BA`、`0x00B5C5AA`、`0x00B817C7`、`0x00CA955E`、`0x01739F3A`、`0x017730C2`、`0x0185856A`、`0x01858664`、`0x0185875E`、`0x01858EAE`、`0x01914351`、`0x019913DF`、`0x0208A9DE`、`0x0214DD92`、`0x0290C85C`、`0x030323A5`、`0x033D6045`、`0x033D8A15`、`0x033D93BB`。配置为 `{0,0,0,0,1,0}`，即 manager `+0x2C=1`、`+0x2D=0`。
- `0x01B63FF7` 先把六字节配置所在 qword 零化，再将 `0xD324EC` 直接用于配置基址 `+4`，得到同一 `{0,0,0,0,1,0}`。
- `0x009C0EE1` 先用默认构造器初始化配置，再由 `0xD32508` 形成临时字节 `{3,1}`、`0xD32512` 复制到配置 `+4`，最后经 `0xD3251C` 克隆后传给 `0xD31686`。其六字节输入为 `{0,0,0,0,3,1}`，即 bound 3 / selector 1。

因此，在这 30 个已找到的**直接 E8 setup 路径**各自被执行且 `0xD31686` 完成其 manager setup 的条件下，静态输入的 loop bound 均非零且 selector 均落在 loop 内：默认配置选择 index 0，唯一自定义配置选择 index 1；两者都会使 `0x217D58C` 的条件 `+0x30/+0x38` pair assignment 分支可达。这里的“分支可达”来自配置值和静态控制流，不证明任一现场调用实际发生、source pair 在现场非空、slot2/slot3 全局调用顺序，或这些 manager 对应 T1–T5 观察到的 dispatcher。枚举只覆盖映像中可识别的直接 rel32 `E8` xref，不把间接调用、条件路径或未解析边界外推为已覆盖。

逐调用点窗口与 helper 导出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`：`r2-d31686-callsite-windows-20261014.py/.txt` SHA-256 `D61918FE955A54A5A715AEFD44BD7BC2E4904D52DCC6A440015FEB025C1A678C` / `1E6A0E4544F26DBCA95C2DB17B3A409E131D37722D94190AA502B205595EDDB9`；配置 leaf `0x4E6E5A/0x4E6DAC/0x4E91A6/0xD32508/0xD32512/0xD3251C/0xD324EC` 的 bounded 解码 `r2-config-helper-leaves-20261014.py/.txt` SHA-256 `F3A047C4F7F5986FF0101B2562EF85E985F0C2C42E7A24938128BE6CCE4C55CE` / `7C34E8DA05BCA9FEFAC0E3FAC414803A05B56CEB4F862E418E36E00CDFD0EE1A`。两脚本均先校验冻结样本 hash；本项不识别任一 runtime manager 的身份、last owner、析构时机、stop/cancel/drain/quiescence、R3 identity 或历史 AV 根因，R4 仍未准入。

### 7.22 ThreadTaskManager 与 TaskQueue 间的弱引用边（2026-10-14，固定样本静态）

对同一冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）按有效 `.pdata` 边界核对 `0x217D16C`、`0x217D35E`、`0x1687C`、`0x168CE`、`0x217D58C`、`0x46448`、`0x75ADB0` 与 `0x31F8F5E`；用 `.rdata` COL/RTTI 识别两种 shared control block。仅分析固定磁盘样本，没有读取/启动 QQ、调用 native 或操作 SnowLuma。

- `0x217D35E` 为 `ThreadTaskManagerImpl` 构造 shared holder：控制块 RTTI 是带 custom deleter 的 `shared_ptr<ThreadTaskManagerImpl>`，manager 自身作为 strong pair 返回。它随后把该 strong pair 转给 `0x1687C`，目标字段为 `manager+0x10`。`0x1687C` 对 weak 来源执行可存活检查/临时 strong lock，再由 `0x168CE` 增加控制块 weak 计数并写入 16-byte pair；`0x168CE` 的释放旧值路径与同组 pair helper 对应。因此 `ThreadTaskManagerImpl+0x10` 是 weak self-reference，不会单独维持 manager 的强生命周期。
- `0x217D58C` 的逐项初始化从 `manager+0x10` 经 `0x46448` 做 weak-lock 形状的 strong acquire；`0x75ADB0` 在 strong counter `+8` 非 `-1` 时原子递增，否则返回空。其结果交给 `0x31F8F5E` 写入 `TaskQueue+0x48`；该位置经 `0x168CE` 更新 weak counter `+0x10`。TaskQueue control block RTTI 为 `__shared_ptr_emplace<TaskQueue,...>`，故此反向字段也是 weak backreference。
- 同一逐项路径另把 `TaskQueue` strong pair（增加 `+8` 后）放入 `ThreadTaskManagerImpl+0x48` pointer-backed vector；命中 selector 时也把 strong pair 交给 `0x106E6`，写入 manager `+0x30/+0x38`。所以这里排除了特定的 `ThreadTaskManagerImpl → manager vector → TaskQueue → manager+0x48` 强引用环；它不识别全局 dispatcher，也不排除 manager 的其他强 owner 或其他类型的环。
- 该发现不证明 manager 的最后一个 strong owner、外层销毁时机、child+0x110 holder 何时释放、所有 TaskQueue/TaskRunner 的 stop、线程 wait 成功、QQ callback queue drain/quiescence、R3 stable identity 或历史 AV 原因。不能单凭弱回边把 child teardown 或 R4 准入判为已闭合。

本机可复核脚本/输出位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`：`r2-manager-weak-selfref-closure-20261014.py/.txt` SHA-256 分别为 `0971FD81D5C8E6A08DEC2F9AE9E88C6F77C8756FE7A308BCA84AB8AF6CCB205D` / `301421BFB9A9C02119117742FA0E0FD135A1340FCFBC559429D4F03A25F0D4D7`。脚本在解析前校验冻结样本 hash；helper 的 counter 语义只按有界机器码得出，不代表现场对象状态。

### 7.23 TaskRunner 的 manager 弱引用与 NTEngine parent release（2026-10-14，固定样本静态）

继续核对同一冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）。只读 PE、有效 `.pdata` 函数和 RTTI；没有读取/启动 QQ、操作 SnowLuma、调用 native 或发送消息。

- `0x217D58C` 从 `manager+0x10` weak-lock 得到临时 strong pair 后，将它传给 `0x31F878C` 初始化 `TaskRunner`。该函数将 pair 的对象/control 指针写入 `TaskRunner+0x8/+0x10`，然后对 control block `+0x10` 执行原子递增；同一控制块布局中 strong count 位于 `+0x8`，weak-copy helper `0x168CE` 也递增 `+0x10`。因此 `TaskRunner+0x8/+0x10` 是 `ThreadTaskManagerImpl` weak pair，不是 strong owner。TaskRunner slot 0/3 的代码从 `this+0x8` 经 `0x3F70` 获取临时可用对象后再读取 manager `+0x58`，与弱引用 lock 路径相符。
- 合并 §7.17/§7.22 的强弱边，已追出的路径是 `ThreadTaskManagerImpl+0x48 vector --strong--> TaskQueue --strong--> TaskRunner --weak--> ThreadTaskManagerImpl`；故这一条曾看似可能的三段环并非强引用环。此结论只覆盖这些已识别字段，不证明没有其它强持有路径、现场对象执行过构造，或 manager 的最后强 owner 已找到。
- `NTEngine` ctor `0x9C0C68` 建立 child holder 并经 `0xD31686` 完成 manager setup；另调用 `0xD31AC0`，其 manager slot 1 以样本 RVA `0x3FAD8D0` 的索引字节 `1` 选出 manager vector 中的 TaskRunner pair，再复制到 NTEngine `+0xE8`。这为该外层对象增加一条独立 TaskRunner 引用。RTTI vtable slot 0 的 thunk `0x9C2C6C` 转入 destructor body `0x9C15D6`；其 cleanup 调用 `0x9C1700`，在 child `+0x110` 与 `+0xE8` pair 非空时对该 runner 经 `0xD32138` 发起 slot-0 虚调用，随后释放临时 pair，并清理/释放 NTEngine `+0xE8` 与 `+0xE0` 字段。固定样本对 ctor 的有界直接 E8 扫描找到 `0x9C1928→0x9C0C68` 初始化路径；没有据此识别 dtor 的实际注册或触发者。
- 上述 parent release 与 runner slot-0 调用是候选 teardown 序列，不证明 NTEngine dtor 实际到达、该 runner/manager 与现场 dispatcher 对应、所有 manager entry 已停止、句柄等待成功或 callback queue 已 drain/quiescent。`TaskRunner` 弱边消除了这条候选 cycle，但 child/manager last-owner、外层 holder 释放时机、R2 consumer/ABI 与 R3 identity 仍未闭合；R4 不准入。

本机固定-hash复核位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\`：`r2-nte-cycle-lifetime-20261014.py/.txt` SHA-256 `01F3881AB2F28E2B6F80CBA60F85AAC9A9C79CE1D93BCD18474A6A81F585E6AC` / `D037AFACF1D797ABE0F3D031D838C68B6A44A1744B63763A54652D08E840BD88`；ctor/dtor xrefs `r2-nte-engine-ctor-dtor-xrefs-20261014.py/.txt` `F95799643D1C66817539FE139A9AB5FC73F321EA7BD1990ABA53BE9D8E535C54` / `E8BCDF065491C96A565B1D7FD191A9DBEC4F77A09A32964AE420468D8A7905A7`；parent pair helpers `r2-nte-parent-field-helpers-20261014.py/.txt` `94A4F3F8FA7547982C343D99DA7D7CD554331EF99F7BF66B6B5D63D2A71087D6` / `C9D22F2D908069A3824FC8BD53A6BF1D4F4E122E8D5861A2D8F5D56A5A670A88`。脚本先校验固定 `wrapper.node` hash；函数/调用者筛查有界，缺失 xref 不能证明无间接路径。

### 7.24 NTEngine 静态访问器调用面与 finalizer 未知（2026-10-14，固定样本静态）

继续在同一冻结 `wrapper.node` 上做有效 `.pdata` 与已解码直接 E8/RIP-relative 扫描；SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`，不访问运行中的 QQ 或 SnowLuma。

- `0x9C1928` 读取 module 的线程初始化 epoch、TEB TLS 与 guard，并在构造路径将静态存储 RVA `0x6750088` 交给 `0x9C0C68`，完成后执行 `0x3E67E74` guard footer 并返回该存储地址。guard 位于 RVA `0x67501E8`。该函数表现为 NTEngine 的 guarded module-static accessor；有效 `.pdata` 中识别到 796 个直接 rel32 `E8` 调用点，说明它是广泛使用的外层对象入口，不证明每个调用发生或其 manager 与 T1–T5 dispatcher 对应。
- NTEngine vtable slot 0 的 dtor thunk 为 `0x9C2C6C`，bounded 直接 E8 scan 找到 0 个调用者；析构主体 `0x9C15D6` 的唯一直接 caller 是该 thunk。访问器反汇编显示 ctor 后调用 guard footer，但没有显式传入 dtor 指针或呈现 registration 调用；这不能排除函数指针/on-exit/runtime-finalizer 或模块退出路径。故 guarded static object 可列为 child manager holder 的外层 owner 候选，但对象最终化是否注册/执行、何时释放 manager last strong owner仍未知。
- 对整个冻结文件中的精确 preferred-VA qword 扫描，`0x18009C2C6C` 仅在 `.rdata` RVA `0x3FAD868` 的 NTEngine vtable slot 0 命中一次；dtor body `0x18009C15D6`、静态 object VA `0x186750088` 与 guard VA `0x1867501E8` 均无 qword 命中。这只约束精确绝对 qword 编码，不排除相对/包装指针、运行时 on-exit 注册或间接 finalizer。
- 本结果补充 §7.23 的 parent `+0xE0/+0xE8` 释放路径，不证明它在现场到达、析构序列成功、manager 所有 TaskRunner/TaskQueue 停止或 callback queue drain/quiescent。R2 ABI/consumer/teardown 与 R3 identity 仍开放，R4 不准入。

本机固定-hash脚本/输出均先校验输入 hash：`E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\r2-nte-static-owner-xrefs-20261014.py/.txt` SHA-256 `4157196D980BDEDC4161A4701FD9C51A306326CEC68F7205D3B8B4CAC2BEDF84` / `4830002B2C977B6ECDD832EAA3398DC2D8DB8228443B0F7AAD5CB265B1BC8F58`；guard helper `r2-nte-init-guard-followup-20261014.py/.txt` `825419BA062ED5C93145B98DE66D6A8FC968317E99F94E5B9A4B86F2147FCB8B` / `6E4D3838B605F7ACFA56300B1400A4359B373C98CB70BA69D1E79139C245C6A6`；精确 preferred-VA qword scan `r2-nte-static-pointer-table-20261014.py/.txt` `0E1193A77227C71963BE391B2305AF9F404FE3BB776D77C147C2AE427FB5BA33` / `EA52C144DEE55422A23BA348F812111B2500A3D3623FA356BD6EFA2B5532BE30`。调用者覆盖只含映像中解码的直接 E8、RIP-relative 代码引用与本次列明的精确 qword 值，不能据此排除间接生命周期路径。

### 7.25 PE TLS callbacks 与析构 RVA 命中分类（2026-10-14，固定样本静态）

继续核对同一冻结 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。只读 PE 导入、TLS 目录、`.pdata` 与数据段；没有读取或操作 QQ/SnowLuma，也没有调用 native 或发送消息。

- PE TLS callback array RVA `0x490A2F8` 登记两个 callback：`0x3E68820` 与 `0x3E67F70`。按 Windows TLS callback reason，`0x3E68820` 只在 reason `2`（`DLL_THREAD_ATTACH`）进入；它计算的固定表 begin/end 均为 RVA `0x490A2A0`，区间为空，故此 callback 在该映像里不会调用表项。其间接 callback helper 位于 `0x3EB1DE0`，机器码仅 `jmp rax`。`0x3E67F70` 处理 reason `0`（`DLL_PROCESS_DETACH`）或 `3`（`DLL_THREAD_DETACH`），从当前线程 TLS 项 `+0xB0` 逆序取 callback 并逐项调用，再推进/释放列表；所以 process-detach 下存在一条间接 TLS callback-list 执行路径。该函数本身没有直接引用 NTEngine static RVA `0x6750088` 或调用 NTEngine dtor，但列表注册者尚未找到，不能排除 NTEngine dtor 被间接放入该列表。
- 导入表中没有名字匹配 `atexit`、`_onexit` 或 `__cxa_atexit` 的项目。此前识别的 guard footer `0x3E67E74` 的间接目标已解析为 `AcquireSRWLockExclusive`、`ReleaseSRWLockExclusive` 与 `WakeAllConditionVariable`，不能据此视为析构注册 API。PE 未单独保留 `.CRT` section；这只限制本次可见的常规注册线索，不证明没有内部/间接 finalizer。
- 对 `.rdata/.data/.tls/_RDATA` 做对齐 DWORD direct-RVA 与 rel32-from-next 搜索时，析构 body RVA `0x9C15D6` 唯一命中在 `.rdata` RVA `0x4C81BC8`。该位置通过析构 body 自身 `.pdata` 记录 `0x9C15D6–0x9C16E9`、UNWIND_INFO RVA `0x4C81B74` 连到 handler data；其中 EH func-info magic `0x19930522` 指向 unwind map RVA `0x4C81BC0`，该命中是析构自身异常/清理元数据中的 action RVA，并非退出注册表项。析构 thunk `0x9C2C6C` 没有在该对齐 RVA/rel32 扫描中命中；精确 VA qword 结果仍限于 §7.24 所列 vtable。
- 对 TLS index RVA `0x6755690` 的有界常见 RIP-relative 访问筛查，1499 个 `.pdata` 有效函数命中候选；按 `GS:[0x58] → TLS index → TLS block` 数据流和寄存器别名筛选后，仅 callback `0x3E67F70` 显式访问 TLS block `+0xB0`，且它在清理时读当前 head 并写回 next pointer。该 pass 未识别注册 writer；覆盖限于脚本列出的指令编码与直接 TLS-block 数据流，不能据此断言不存在经 wrapper/helper 传递的 producer。
- 因此进程退出 callback 的动态列表仍是候选间接 finalizer 通道；NTEngine module-static destructor 是否注册到该列表、是否经其他内部/包装机制触发、实际执行时机和 manager last-owner 仍 unknown。唯一静态析构 body RVA 命中已归类为自身异常处理元数据。TLS 观察不证明 teardown 安全，也不闭合 manager 全任务 stop、callback drain/quiescence、R2 consumer/ABI 与 R3 identity；R4 仍不准入。

本机复核脚本和输出均先校验固定输入 hash：`E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\r2-nte-finalizer-callbacks-20261014.py/.txt` SHA-256 `9F7ECEA08E80E93F9C0FDEA381D5ECE02EDFFA0E0FB308579FCD6B7197087415` / `7961CE0BD7A817ECD36CADBFEA2AA8DF8622EE80C3EC3E4B086470DCAD2EA50B`；对齐 RVA/rel32 扫描 `r2-nte-relative-finalizer-refs-20261014.py/.txt` `8ADECBB4A43BD670D184E71EEE2881AA7E78B9F20A72E87587CF04084ED672A2` / `91E49F940B139FB133D0F5BF2682DC23F88807DB4ADE74355E10DD3A1AAC3058`；析构异常元数据分类 `r2-nte-dtor-rva-eh-classification-20261014.py/.txt` `B70EF37673DF52362765D6B5B6AA08D540855BC9A347E6922108BC1EBC979CBF` / `52C38B6F5F67388E8054608AFA10A116271FA6B9498D9D6407217E13C31A7E26`；TLS index/block `+0xB0` 数据流筛查 `r2-nte-tls-list-dataflow-20261014.py/.txt` `9F941B1E03E9B99D92BDEC8D7B7981A47788C12BFFF5928CD207DDCB692DB812` / `7F77AB4D9ECB647F4049C2917315BBDC248A64BC5A6C8AA730CBA672E2F1141B`。扫描有明确 section 与编码边界，不覆盖未列出的包装指针或运行时状态。

### 7.26 TLS detach callback dispatch, node release 与 producer 有界追踪（2026-10-14，固定样本静态）

继续使用冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）做 PE/`.pdata`/Capstone 静态分析。未读取或操作 QQ/SnowLuma、未调用 native、未发送消息。

- `0x3E67F70` 在 `0x3E67FC5` 从当前 TLS 节点取 callback 地址到 `RAX`，随后 `0x3E67FCD` 通过 `.rdata` RVA `0x4909B18` 的 qword 调用 `0x3EB1DE0`；该 helper 的完整函数体只有 `jmp rax`。节点首 DWORD 控制逆序遍历的 callback 项，节点 `+0x8` 是随后读取的 next pointer。代码调用 `0x3E94F90` 释放已处理节点，再把 TLS `+0xB0` head 推进到 next node。
- `0x3E94F90` 没有独立 `.pdata` 项，机器码为跳转 thunk `jmp 0x3EA01D0`。有效函数 `0x3EA01D0–0x3EA020C` 将输入指针作为 `HeapFree` 的第三参，调用 KERNEL32 `HeapFree`；失败路径调用 `GetLastError` 并把结果写入 CRT 错误槽。这里确认了 detach list 的 callback dispatch 与节点释放路径，未给出节点注册者。
- 对 TLS index RVA `0x6755690` 的 1499 个 `.pdata` 候选扩展 `mov/lea`、寄存器别名、常数位移及 Win64 参数传播追踪。候选函数产生 37 个传递 TLS block/pointer 的直接调用边；继续沿 helper 追到 46 个直接转发边、50 个函数/参数状态。该范围仍只在 `0x3E67F70` 发现 TLS block `+0xB0` 读写，没有 helper 写入。8 个间接调用位点均解析到系统导入：`AcquireSRWLockExclusive`、`GetCommandLineA`、`GetLastError`、`SetLastError`。
- 本轮没有定位 TLS list producer。数据流范围受 TLS-index 指令模式、常见寄存器/`lea`/加减追踪和直接调用边限制；未覆盖栈 spill、任意计算地址或运行时改写函数指针形成的所有路径。不能据此证明不存在其他 writer，也不能证明 NTEngine 析构已注册到 detach list 或会在模块退出时执行。

固定输入 hash 校验与导出脚本/输出保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\`：`r2-nte-finalizer-helper-detail-20261014.py/.txt` SHA-256 `4CEEAC3702F52718131C26F90F1CCFD6D33631E33B7CE7BF08651076790824EA` / `277211C674A7F6A846FBB18E8A3449329A3B867F61DF9C78CA0D0323FBC01FD9`；`r2-nte-indirect-targets-20261014.py/.txt` `C99B976C58FCDED904082D424D005601B0992DE297355E1B951A0D59167B8D6D` / `ED3854B333BC58CF7C5C91839F7341CEC06AAF2A198E47233163616DD0206116`；`r2-nte-tls-producer-dataflow-20261014.py/.txt` `61396632EBF203C51D5945CE4542D7B78D7696334244AB6FB8942367C9FA9994` / `356F1F9773EEF265CD0038F13E926A8D90526A8F7E602C20F5D1BD48065FB295`；`r2-nte-tls-helper-bfs-20261014.py/.txt` `968074BDE5A4751DA4C6568D2DE2DFFA8E6ED1C3757C58C44345E8C2E7779437` / `B3F72D88761F4AE7F667C1280C15890B3A796AB6CEC9B7D4D924750FD4329A6A`。分析结论不关闭 manager last-owner、`child+0x110` holder release、全任务停止/排空、R2 consumer ABI/thread、R3 stable identity 或历史 AV；R4 仍不准入。

### 7.27 NTEngine `+0xE0` raw owner 到 child destructor 与 manager-holder release（2026-10-14，固定样本静态）

继续核对同一冻结磁盘 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。使用 Capstone 5.0.7 复核有效 `.pdata` 函数与无 `.pdata` 的 helper 基本块；没有启动/读取 QQ、操作 SnowLuma、调用 native 或发送消息。

- 构造 helper `0x9C12A2` 通过 `0xB75000` 请求 `0x120` 字节，并把返回地址经输出槽交给 `NTEngine` ctor `0x9C0C68`。随后 ctor 将该 raw pointer 写入 NTEngine `+0xE0`，并将它传给 `0xD31686` 建立 child manager。helper 还在 `0x9C1309` 调用绝对 VA `0x180D3111E2`；该地址在冻结 `wrapper.node` 映像外，当前证据不足以归属模块或解释其语义，不能由假设 load base 推出 QQNT 归属。
- NTEngine dtor body `0x9C15D6` 在 `0x9C16B8` 把 `this+0xE0` 交给 `0x4E6872`。该无 `.pdata` helper 清零 RDX 后转入 `0x4E91A6`；后者先清空 owner 槽，若旧指针非空则进入 `0x4E91BE`，调用 child dtor `0xD31356`，随后 tail-call `0xB75029` 释放旧对象。因此固定样本静态析构路径已把 NTEngine `+0xE0` raw owner 接到 child destructor。该路径仍以 NTEngine dtor 实际执行且 `+0xE0` 非空为条件。
- child dtor `0xD31356` 先调用 `0xD313D6` stop/wait 路径，然后在 `0xD31371` 对 `child+0x110` 调 `0x1DB2` 释放 manager shared holder。这个释放点现在已静态定位；结合 §7.15–§7.16 的限制，它不证明 wait 成功、manager 全部 task 已停止、QQ callback queue 已 drain/quiescent，或该 holder 是 manager 最后一个 strong owner。
- NTEngine `+0xE8` 是单独 pair 字段：dtor 在 `0x9C16AC` 直接调用 `0x1DB2`；不把它与 `+0xE0` raw owner 或 child 基址 `+0x110` manager pair 合并。此前 §7.23 的 runner teardown 路径仍是另一条独立证据。
- 因而已知的是“条件性 parent `+0xE0` 清理 → child dtor → child `+0x110` holder release”代码边；仍未知的是 guarded NTEngine static destructor 是否注册/实际触发、外层 owner 何时销毁、manager 最后一个 strong owner、R2 consumer ABI/thread 与全局 stop/drain/quiescence、R3 stable identity 和历史 AV 根因。R4 仍不准入，feat-005/006 与产品 NOT READY 状态不变。

本机固定-hash复核脚本/输出位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\`：`r2-nte-parent-child-release-chain-20261014.py/.txt` SHA-256 `71AC9C97880E2CDB3A6C666008BAE2737516838A83098138D8F56B78D70CD000` / `60FC9433BC72619EE95924493AFFDECB6C9497E0CECB1F7F0839AB9333BEEFCF`。脚本先验证输入 hash；导出列出 `0x9C12A2` allocation/call、NTEngine ctor/dtor 窗口、完整 `0xD31356` child dtor 与 raw helper blocks。`.pdata` 定界和 helper 基本块分析均有限，不凭缺失调用者证明析构已触发或不存在其他 owner 路径。
### 7.28 `MSFThread_xpng` owner path to `ThreadTaskManagerImpl` (2026-10-14, fixed-sample static)

Using the same frozen disk `wrapper.node`, SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`. The bounded Capstone scans assert this exact hash; no QQ process or SnowLuma interaction occurred.

- The image has 525,295 valid `.pdata` function ranges. The earlier census found 30 direct rel32 `E8` calls to `0xD31686`; only the direct-call chain `0xD31686 → 0x217D16C → 0x217D242` was found for manager factory/constructor targets in that pass. It also found three direct `D31356` call sites; two belong to the `MSFThread_xpng` construction-replacement and deleting-destructor paths below. These are direct-code findings, not coverage of indirect calls.
- `0x1B64310` builds the shared pair for an inline `MSFThread_xpng` object. Its RTTI vtable at RVA `0x4141408` names `.?AVMSFThread_xpng@MSF@@`; slot 0 is `0x1B643C0`, slot 2 is `0x1B63FC0`. `0x1B63EC0` initializes the wrapper's `+0x8` payload pointer by allocating `0x120` bytes and calling the absolute target `0x180D30FF0`; the then-current outside-image classification is superseded by the 2026-10-15 correction in §7.70. Constructor argument semantics remain unknown.
- Vtable slot 2 `0x1B63FC0` reads the payload at `this+0x8`, obtains the six-byte default manager config through `0xD324EC`, calls `0xD31686(payload, config)`, then calls `0xD31A9A(payload)`. The established `D31686` contract places the manager holder at `payload+0x110`; this is a second static owner shape beyond the NTEngine `+0xE0` raw-child path in §7.27.
- `0x1B64310` has three verified direct callers: `0xB90C40` at `0xB90DE8` and `0xB90E7E`, and `0x1BBF810` at `0x1BBF95B`. In the first function, the returned pairs are stored at global pair addresses `0x677A2D0` and `0x677A2E0`; the same function then calls vtable offset `+0x10` at `0xB90E5E` and `0xB90EF4`. The third caller stores the returned pair in its owner at `+0x8` and calls vtable offset `+0x10` at `0x1BBF9C8`. Those offsets match the RTTI-identified slot 2. This proves the static setup calls for these three constructed wrappers; it does not establish their runtime execution or lifetime.
- `0x1B643C0` is the `MSFThread_xpng` deleting-destructor path: it clears the wrapper's `+0x8` payload pointer, calls `0xD31356` when non-null, frees the payload, and conditionally frees the wrapper. The assignment helper `0x1B63EC0` likewise calls `D31356` before freeing an overwritten non-null payload. Thus the path from an outer `MSFThread_xpng` shared owner to `payload+0x110` manager release is statically present, conditional on the wrapper release/replacement path actually executing.
- This closes an additional manager-holder owner shape but not the last strong owner of any observed manager, finalizer registration/trigger, a successful wait, all-task stop, callback queue drain/quiescence, response-consumer ABI/thread, stable native identity, or the historical AV. The constructor at `0x180D30FF0` remains unbound to an object type/argument contract; see the 2026-10-15 §7.70 correction for its exact image scope. Do not infer a QQNT base or constructor contract. R1–R3 remain open, R4 is not admitted, and product readiness stays NOT READY.

Reproducible local-only artifacts under `E:\stella\_reference\n4-r2-callback-manager-static-20261013\nte-engine-owner-followup-20261014\` (each script asserts the frozen wrapper hash):

- factory census `.py/.txt`: `5641C34341A52649AF35AA0006750AB8633576F3B5E7835D76B3DB342393AC72` / `010E96D89C8FD99531A71F1F090F64EF16F185D3D85CEED20B51A5320C679740`;
- D31356 callers `.py/.txt`: `48AAC286A4E61F722CB9EF97481F004ACE7AFF66386F3AA862013DBD01CD8D8F` / `41495D7904A4E1D0F94BD31144F7A01484AC744413CB17E824D62289D30ED3E6`;
- RTTI/control context `.py/.txt`: `ABEEC04D01D17AB18691EDFC7EA9C7A987EA163F9948CF9C3C07C6E880794E6B` / `86FAE7E168D8F26F6CA0772FDC57AD311D3EE48B61FD6F6BFD523691D61AC616`;
- factory direct callers `.py/.txt`: `75A0D4BD99ADBBB5C458FF75FD32F71390043C0D78C3B56810470E2D6488D631` / `E22B1A26E3B3C5DBB2CE1F5EE249D4F3E790AF3D0535355648F6950ACD03E672`;
- manager setup method `.py/.txt`: `FA211374C6B6F9B293F8D6CA4F9680CF1D5FB494CA45F57F58A460A5E5A0DED1` / `48AF450B0A11D0FE4884DE0DEF5A16030567A9556A2741FD4BD3636C14A77005`;
- vtable method references `.py/.txt`: `E8DB92387F93C5C5A084D2BA39CF8BA1A6BEF7750C92CC796AFB8C09CAF0B2DE` / `82D6B7981C49CE25F978A557B7C97D7DB4DEC8878D41D715768BF0C6B1B555C1`;
- vslot-2 callers `.py/.txt`: `AFB80F0AED9A8801721A074553E653A597004B06B644E1BDE9A9E392C868022A` / `03481711C4C31BD7B050DC612858E065D3FA926404C93C48CCD30EA924514098`.

### 7.29 `ThreadPoolImpl` callback-manager owner-stop path (2026-10-14, fixed-sample static)

The same frozen `wrapper.node` was rechecked by bounded Capstone decoding after validating SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`.

- RTTI identifies the vtable at RVA `0x4300918` as `.?AVThreadPoolImpl@internal@xpng@@`; slot 2 is `0x290C746` and slot 0 is `0x290DA46`. In slot 2, `0x290CC0F` produces a local pair-shaped value, `0x2EC8` assigns it to `this+0xA0`, and the method loads the field's first qword before direct call `0x290C85C → 0xD31686`. This is a type-specific manager setup edge; the pair pointee's dynamic identity is not established here.
- Slot 0 reaches `0x290C59C → 0x290C612`. If the `this+0x20` state check through `0x69A08(..., 2)` takes its cleanup branch, `0x290C612` passes `[this+0xA0]` to `0xD313D6`, then calls `0x504AEC` on the pair field. That helper copies 16 bytes, zeroes both source qwords, and releases the copied pair through `0x1DB2`; the outer cleanup wrapper also reaches `0x1DB2` for `this+0xA0` after the conditional base cleanup.
- This adds a concrete `ThreadPoolImpl` setup-to-stop/reset/release path. It remains conditional static code: it does not prove an instance reached setup or teardown, that `+0xA0` is the final strong owner, that `D313D6`'s wait succeeded, that all callback work drained, or that this relates to the historical AV. The manager payload RTTI, indirect consumer ABI/thread, dispatcher-wide quiescence, and stable native identity remain open; R1–R3 stay open and R4 is not admitted.

Local fixed-sample artifacts in `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\` (the script asserts the frozen image hash): `r2-threadpool-manager-owner-stop-chain-20261014.py/.txt`, SHA-256 `C5D69CE9ABCC3BD1EC657FB815F401478B105A60EDF1F92C029128EC9202F8EE` / `A0AE6A23EB7117F3A35F09443E96E22AB4A4F1477BC853FE148F86F86DFCC9BE`.

### 7.30 `FileDragManagerWin` owner-to-child destructor path (2026-10-14, fixed-sample static)

The same frozen `wrapper.node` hash was checked by a separate bounded Capstone/RTTI script: `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`.

- RTTI identifies the vtable at RVA `0x3FADD38` as `.?AVFileDragManagerWin@nt@@`; slot 2 is `0x9C4852`, slot 0 is `0x9C5462`. Slot 2 transfers the first qword from a local pair-shaped value into owner field `this+0xA8`, reloads that field, and calls `0x9C4951 → 0xD31686` with the default manager config. The manager holder is installed in the child payload at `+0x110`; this static path does not identify the payload's dynamic RTTI.
- Slot 0 reaches `0x9C47DE`, which clears the owner field through `0x4E6872 → 0x4E91A6`. If the old payload pointer is non-null, `0x4E91BE` calls `0xD31356` on it and tail-calls `0xB75029` to free it. The established `D31356` body reaches `D313D6` and releases the child `+0x110` manager holder.
- This is an additional static owner-to-child teardown path. It does not prove runtime construction or destructor execution, manager last-owner status, successful wait, all-task stop, callback drain/quiescence, response-consumer ABI/thread, stable native identity, or the historical AV cause. R1–R3 remain open and R4 remains not admitted.

Local artifacts in `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\` (the script asserts the frozen image hash): `r2-filedrag-manager-owner-stop-chain-20261014.py/.txt`, SHA-256 `EAE21E4CE8660D1C3852DF941D8C6517AE222F86DF27924A76450183E7B8A66A` / `FBF5CD676007342D1E0A3D2D97170A0D1BD56BFD262CA8D050E37DD85C7B837C`.

### 7.31 Four manager fields with matching setup and stop/update methods (2026-10-14, fixed-sample static)

The fixed `wrapper.node` SHA-256 remains `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`.

- Four bounded setup methods use matching `this`-relative manager fields: `0x18584E8` checks and populates `+0x1D0`, `0x18585E2` `+0x188`, `0x18586DC` `+0x1B8`, and `0x1858E2C` `+0x1A0`. Each loads the field's first qword and calls `D31686` with a local pair-shaped configuration. These exact fields are also checked and passed to `D313D6` by method `0x1852D42`; its cleanup order is `+0x188`, `+0x1A0`, `+0x1B8`, `+0x1D0`. The evidence supports a four-slot owner layout and a stop/update path, not a destructor claim.
- The stop/update method has 22 direct E8 callers across 16 function entries. The fixed-image direct `D313D6` census contains 18 callsites across 12 caller functions. Two other methods, `0x33D89AC` and `0x33D9304`, contain both a `D31686` setup call and conditional/error-path `D313D6` calls on the same local manager pair. A bounded exact function-pointer/RTTI scan did not identify a dynamic class name for the four-slot methods; indirect callers and code outside the frozen image are out of scope.
- This does not prove a runtime instance, actual teardown trigger, manager final strong owner, successful wait, full task stop, callback drain/quiescence, response-consumer ABI/thread, stable identity, or the historical AV cause. R1–R3 remain open and R4 is not admitted.

Reproducible local scripts and outputs under `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\` (each asserts the fixed image hash):

- Four-field setup/stop disassembly: `r2-parent-multislot-manager-setup-context-20261014.py/.txt`, SHA-256 `692C692D337D5EB8CF407F482D625AAF135DAD84851E82E818E39DFD18700D7D` / `9F2B33C7F2EB420441DC5B23B43BF4A5D314E3B5A72E5ABD205A70448BB15993`.
- Direct callers and data-pointer/RTTI screen: `r2-multislot-owner-method-xrefs-20261014.py/.txt`, SHA-256 `52020025E8D85AFCD107831715FED9C4C5C5B05B419CD5482DB78EFE7DDFE30C` / `9CD54F29B68EF878756BA5C6B0FEB9DEB7165156FD45C1FE9E94F38E4031B07D`.
- Direct stop-call windows: `r2-d313d6-stop-caller-windows-20261014.py/.txt`, SHA-256 `498137BBA7DA679CE7FABC337D0C933629AB80C72D945D592A397585F9752181` / `A6F37D3AF42D69006F8DEA7AC45FD835F97137638F162B6B0F5C31E4C5B12A9C`.

### 7.32 `FileBridgeHostServiceImpl` field-to-four-slot stop/update path (2026-10-14, fixed-sample static)

继续复核同一冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`），只做磁盘 PE/RTTI/Capstone 静态读取。

- RTTI 将 `0xA18CB4` 映射为 `FileBridgeHostServiceImpl@wifiphoto@nt` secondary-vtable slot 4；COL offset 为 `+0x20`，所以函数收到的 subobject `this+0xD8` 对应 complete-object `+0xF8`（布局复核见 §7.33）。同一 secondary vtable slot 13 的 `0xA19830` 也是 `0x1852D42` 的直接 caller。
- slot-4 函数检查 subobject `this+0xD8` 是否非空；若非空，取该 16-byte pair 首 qword 为 `RCX`，保留入口 `EDX`、设置 `R8B=1` 后调用 `0x1852D42`，返回后经 `0x504AEC` reset/release 同一 pair。故固定样本支持条件性路径 `FileBridgeHostServiceImpl complete+0xF8 pair → four-slot stop/update → pair reset/release`，不命名 pair pointee。
- 固定 hash 的可复核产物位于 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`：`r2-multislot-stop-caller-rtti-20261014.py/.txt` SHA-256 `78C60E446FD9DF53B2A83F506294FAB66BD93C80EDF18FDE2C7C796CDC5F7197` / `675C4F69D3B18447D3D8832A6E3D65F5ED99FD316CBCD042910F05BEFEA0EC20`；`r2-filebridge-host-slot-context-20261014.py/.txt` `1877FE18FBDAC4EBD67A6B3F838BC7015647B4BBD0B27FD5B7B28620637C5973` / `800B496261F5441832EB727570086415A24FB3A29FBDF2493C1AF4FCB1DE917E`。前一脚本复核四槽 stop/update 的 16 个直接调用者对应的精确数据指针；后一脚本输出该 FileBridge RTTI vtable 相关 `.pdata` 函数完整 disassembly。
- 该静态路径不证明 slot-0 是析构/finalizer、任一清理路径实际到达、pair 是 manager 最后 strong owner、wait 成功、所有 task 停止、callback drain/quiescence、consumer ABI/thread、R3 identity 或历史 AV 根因。间接调用与映像外路径未覆盖；R2/R3 未闭合，R4 不准入，feat-005/006 blocked，产品 NOT READY。未访问 QQ/SnowLuma、未调用 native、未发送消息、未运行 Cargo tests。

### 7.33 `FileBridgeHostServiceImpl` secondary-vtable offset 与 `+0xF8` pair field (2026-10-14, fixed-sample static)

继续使用同一冻结 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`，校验同类两个相邻 vtable 及对该表的代码引用。

- primary vtable RVA `0x3FB7708` 与 secondary vtable RVA `0x3FB7718` 的前置 COL 都命名 `.?AVFileBridgeHostServiceImpl@wifiphoto@nt@@`；COL offsets 分别为 `0` 与 `0x20`。因此 slot 4 的 `0xA18CB4` 收到的是 secondary subobject `this = complete + 0x20`，其 `this+0xD8` pair 地址等于完整对象 `+0xF8`。同一 secondary 表 slot 13 为 `0xA19830`。
- `0xA174D4` 的静态 setup 写 primary vptr 到 `[this]`、secondary vptr 到 `[this+0x20]`，并把 `[this+0xB0]` 与长度 `0x88` 交给 helper `0x3EB2680`；该范围覆盖 complete-object `+0xF8`，但此处不单独命名 helper 的类型初始化语义。`0xA17854` 也写这两个 vptr，并在其函数路径对 `[this+0xF8]` 调 `0x1DB2` pair-release。该静态字段初始化范围与清理调用为 §7.32 的 slot-4 `complete+0xF8 → 0x1852D42 → 0x504AEC` 路径提供地址交叉核对；`0xA17854` 不据此命名为 destructor，也不能证明两条清理路径何时执行。
- 因此已识别固定样本中的 typed outer field 以及它的两个静态 cleanup callsite，但 pointee 动态类、最后 strong owner、实际析构/stop trigger、wait 成功、所有 task stop、callback drain/quiescence、consumer ABI/thread、R3 identity 与历史 AV 仍未知。R2/R3 不完整，R4 不准入，feat-005/006 blocked，产品 NOT READY。

固定样本复核脚本与报告在 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`：`r2-filebridge-col-offset-20261014.py/.txt` SHA-256 `8B8712D101E63426D5488407C1E008694CAC1C90AEAE760762622DB4353D7A8C` / `F5E2EB8825D6BA1660CBEA431C277471E5272D20B2FDD257B058C80393A14F15`；`r2-filebridge-vtable-writers-20261014.py/.txt` SHA-256 `82F914F4C570740262EF42F951E4C2F454D6401D2B537BBC36A519E33AF43C26` / `77C8A527BBD3A399A181163EBAE94DB5B8BAD7689036C62960A6091A97FAC581`。脚本均断言精确 wrapper hash；writer scan 对候选 RIP-relative LEA 做 Capstone 与 `.pdata` 边界复核，范围不覆盖运行时/映像外或间接写入。

### 7.34 four-slot stop/update callsites do not consume a return status (2026-10-14, fixed-sample static)

复核 §7.31 的同一固定样本 `wrapper.node` 与完整 `0x1852D42` 反汇编，重点查看每个 manager-stop call 之后的返回值数据流。

- `0x1852E41` 对 `+0x188` 调 `D313D6` 后，下一条控制判断是比较 `[rsi+0x1A0]`；`0x1852EDB` 对 `+0x1A0` 调用后转而读取 `[rsi+0x1B8]`；`0x1852EEC` 对 `+0x1B8` 调用后读取 `[rsi+0x1D0]`；`0x1852EFD` 对 `+0x1D0` 调用后检查保存的输入标志 `BL`。四处均没有消费/测试该调用的 `RAX` 返回值，且 flags 分支来源是后续 `cmp/test`。
- 这证明该父级四槽调用点没有显式检查或传播 manager-stop 的返回状态；不证明 `D313D6` 没有阻塞等待，也不构成线程退出确认。已知深层 `D313D6 → D3355C` 路径执行 `WaitForSingleObject(INFINITE)` 后忽略 wait result，见 §7.15–§7.16；所选 index-0 虚调用的任务类型与全局 callback drain/quiescence 仍未知。
- 复核使用 §7.31 的 `r2-parent-multislot-manager-setup-context-20261014.py/.txt`（SHA-256 `692C692D337D5EB8CF407F482D625AAF135DAD84851E82E818E39DFD18700D7D` / `9F2B33C7F2EB420441DC5B23B43BF4A5D314E3B5A72E5ABD205A70448BB15993`），并用 `r2-d313d6-stop-caller-windows-20261014.py/.txt`（SHA-256 `498137BBA7DA679CE7FABC337D0C933629AB80C72D945D592A397585F9752181` / `A6F37D3AF42D69006F8DEA7AC45FD835F97137638F162B6B0F5C31E4C5B12A9C`）核对 caller windows；两脚本均断言同一固定 hash。结论只覆盖固定映像中该函数的直接静态指令流。
- R2 last-owner/完整 teardown 与 global callback quiescence、consumer ABI/thread、R3 stable identity 和旧 AV 仍未闭合；R4 不准入、feat-005/006 blocked、产品 NOT READY。未访问 QQ/SnowLuma、未调用 native、未发送消息、未运行 Cargo tests。

### 7.35 Direct parent call graph for four-slot manager setup methods (2026-10-14, fixed-sample static)

继续核对同一冻结 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Capstone 5.0.7 按有效 `.pdata` 函数边界验证 rel32 `E8` 指令；未访问 QQ/SnowLuma。

- `0x185806A` 将入口 `RCX` 保存至 `RDI`，随后在 `0x18580A3`、`0x18580B2`、`0x18580BE` 分别以同一 `RDI` 为 `RCX` 调 `0x18584E8`、`0x18585E2`、`0x18586DC`。这三项是 §7.31 四槽 setup 方法中的三个；高层 helper 确实向同一入口对象指针调用它们。该函数还调用 `0x1858493` 并管理局部 pair，但本记录不推断其类型或完整构造语义。
- 另一 helper `0x1858D66` 将入口 `RCX` 保存至 `RSI`，在 `0x1858D86` 和 `0x1858D9E` 以同一 `RSI` 分别调用 `0x18585E2`、`0x1858E2C`。有界 caller census 找到 `0x185806A` 的两个直接调用点 `0x1857288`（函数 `0x18570AC`）与 `0x1859D79`（函数 `0x1859B1C`）；`0x1858D66` 的直接调用点为 `0x18588D3`（函数 `0x1858889E`）。
- 这把三个既有 setup 方法接到一个上层 helper，并为另两个 setup 方法找到独立 helper；它不证明两个 helper 操作同一运行时实例，也没有将这些 helper 识别为特定 RTTI 类、析构函数或 finalizer。§7.31 的条件性 stop/update 与 pair release 结论不变；manager last strong owner、实际触发、成功等待、完整 task stop、callback drain/quiescence、consumer ABI/thread、R3 identity 与历史 AV 仍未闭合。

本机固定-hash产物在 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\manager-method-scope-20261014\`：caller xref `r2-multislot-parent-xref-verified-20261014.py/.txt` SHA-256 `606A317DB4B560BCFB0E4207F669C33102A011A44E7F762186CB5E6277A43528` / `63AC42CA2D6A81BACA60B107E5A97549976BBF533CE00F10342DDC967CE84055`；selected function disassembly `r2-selected-owner-context-20261014.py/.txt` SHA-256 `B80F0C824ABD3E64C312D597F7A3C73C686DEEFC9DBEFADD2CCCE1C04A751AD0` / `3FC4887E2B4FF7C307C80AD76DA1291ED7DD43979730D0EA995059E832C35481`。两脚本均先校验输入 hash，范围是有效 `.pdata` 中的直接调用与所选函数机器码；不能据此排除间接调用、映像外调用或未知动态类型。R1–R3 仍开放，R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.39 TaskQueue/TaskRunner 弱 pair 与线程字段归属更正（2026-10-15，固定样本静态）

重新沿同一冻结 `wrapper.node`（SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`）逐指令复核 `TaskQueue`、`TaskRunner` 构造、manager slot 1/2/3 与 `D32138`。本节 supersede §7.17 中 `TaskRunner+0x8/+0x10` 指向 manager 的判断、§7.18 将 `0x31F88EA` mismatch 投影归给 manager 的判断、§7.23 对回边与 TID 字段的判断；亦把 §7.38 `B7CE8A` state path 的未知 slot-0 对象收窄为已识别类。此前日期化记录保留为当时结论，不再用于当前对象布局推断。

- `0x217DEEA` 先由 `0x217DF64` 构造 `TaskQueue` 对象；其 ctor `0x31F8E2C` 将对象首 16 字节清零。`0x217DEEA` 随后以对象地址（control block `+0x18`）调用 `0x1676E(out_pair, TaskQueue*, control)`。`0x1676E → 0x1687C` 在 `TaskQueue+8` 为空时调用 `0x168CE(dst=TaskQueue, {TaskQueue, control})`，把 self weak pair 写到对象首 16 字节；`0x168CE` 对 control `+0x10` 计数。control-block vtable RVA `0x42606D8` 的 COL/RTTI 是 `__shared_ptr_emplace<TaskQueue@internal@xpng>`。
- `0x31F8F5E` 的首个 pair 操作把输入的 manager pair weak-copy 到 `TaskQueue+0x48/+0x50`；随后它以 `0x46448` 从 TaskQueue 对象首部锁取 `{TaskQueue, control}` 临时 pair，并把该 pair 交给 `0x31F9016 → 0x31F9B36 → 0x31F9BBA → 0x31F9C3C → 0x31F878C` 构造 `TaskRunner`。`0x31F878C` 将这两个 qword 写到 `TaskRunner+0x8/+0x10` 并递增 TaskQueue control `+0x10`，所以这里保存的是指向 `TaskQueue` 的 weak pair，而非指向 `ThreadTaskManagerImpl` 的 pair。`TaskQueue+0x38/+0x40` 保存的是 `TaskRunner` 的 strong pair；`0x63EC4` 从该字段复制时递增 TaskRunner control `+0x8`。
- `TaskRunner` object vtable RVA `0x43B6088` 的 RTTI 仍为 `.?AVTaskRunner@TaskQueue@internal@xpng@@`，slot 0=`0x31F8CEE`。slot 0 经 `0x3F70` weak-lock `TaskRunner+8/+0x10` 到 `TaskQueue`，再调 `0x31F88EA`。`0x31F88EA` 仅当 R9 辅助状态不是 `{qword=0, DWORD=0}` 空状态（即任一字段非零）时进入 `GetCurrentThreadId (0x7879D0)` 对 `[TaskQueue+0x58]` 的比较；slot 3 `0x31F8C7E` 则直接比较该字段。manager slot 2 `0x217D8CC` 遍历 `+0x48` vector，选出各 `TaskQueue` 后调用 `0x31F90B0`，由后者写入 `TaskQueue+0x58`。因此该线程字段属于每个 `TaskQueue`，不是 manager 或 TaskRunner。`D32138` 会先清零 slot 0 收到的 R9 辅助状态；经此包装的 slot-0 调用走 `0x31F88EA` 空状态分支，不经过该 TID 比较。
- 在 `0x31F88EA` 可达的非空辅助状态 mismatch 分支，当前 `TaskQueue` 被传给 `0x63EC4`；该 helper 读取 `TaskQueue+0x38/+0x40` 并强-copy 出 `TaskRunner` pair，然后将首 qword 交给 `D32138`，后者作 slot-0 虚调用。此 nested target 的固定映像动态类仍是 `TaskRunner`；不再把它解释为读取 manager `+0x38/+0x40` 的跨字段 projection。该调用携带的 closure/task 后续效果仍须从其参数消费者单独证明。
- 与之相接，`0x217D58C` 的 selector pair `manager+0x30/+0x38` 指向配置选出的 `TaskQueue`；其构造来源 `0x217D6D8 → 0x217DEEA` 与上述 control-block RTTI 一致。slot 2 `0x217D8CC` 从该 `TaskQueue` 的 `+0x38/+0x40` 取 `TaskRunner` pair 并安装 TLS holder；slot 3 `0x217DA96` 也从相同 `TaskQueue` 投影该 `TaskRunner` pair。所有权字段的静态图为：manager vector 与 selector pair strong-hold `TaskQueue`，`TaskQueue+0x48` weak-backreferences manager，`TaskQueue` 首 16 字节 weak-self-reference，`TaskQueue+0x38` strong-holds `TaskRunner`，而 `TaskRunner+0x8` weak-backreferences `TaskQueue`。这条路径没有强引用环，但不排除其它 manager owner。
- `B81758` 对 manager 使用 `{bound=1, selector=0}` 配置并以索引 0 调 `D31AC0`；返回的 `TaskRunner` pair 存在 B816E8 所指全局状态 `+8`。`B7CE8A` 读取该 pair 并调用 `D32138`，故这条 MSFMessenger callback state path 的 slot-0 对象静态类型为 `TaskRunner`。`D32138` 清零其传给 slot 0 的 R9 辅助状态，因此本路径经 `0x31F88EA` 空状态分支，不读取 `TaskQueue+0x58` 做当前 TID 比较；其路径继续进入 `0x31F90C6`，细节见 §7.40。这将 §7.38 的该条路径从“动态对象类型未知”更正为类型已知；不能推广到 `D32138` 的其它调用者或据此证明现场 callback 消费、实际线程匹配、排队成功或 quiescence。

固定-hash Capstone follow-up `r2-taskqueue-taskrunner-self-pair-correction-20261015.py/.txt` 保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\response-consumer-followup-20261014\`，SHA-256 分别为 `0272B0291F3749193C508822CC761D0F88EB5847F6AD0287D0504E6775854C60` / `16AF056A45F12D237EB9C3052963A3400E298AA178FD199264335ACB8F4F3A0F`。脚本先校验固定样本 hash，导出指定有效 `.pdata` 函数、TaskQueue/TaskRunner control-block RTTI 及 `0x63EC4` 有界字节窗口。

本更正只闭合固定映像中的 selected TaskQueue/TaskRunner owner 与 `+0x58` 字段归属，以及 `B7CE8A` state path 的 slot-0 类名；不证明该构造路径在目标实例执行、global dispatcher 与该 manager/vector/TLS 项映射、closure 的业务效果、manager last strong owner、全项 stop、HANDLE wait 成功、callback drain/quiescence、R3 stable identity 或历史 AV 原因。R1–R3 gates 仍开放，R4 不准入，feat-005/006 blocked，产品 NOT READY。未访问 QQ/SnowLuma、未调用 native、未发送消息、未运行 Cargo tests。


### 7.40 D32138 TaskRunner slot-0 空状态路径与 TaskQueue+0x18 投递（2026-10-15，固定样本静态）

继续复核 §7.39 的 `B7CE8A→D32138→TaskRunner slot 0` 路径，以及其准备的 record 后续进入何处。输入仍为冻结 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；函数按有效 `.pdata` 解码，相关非 `.pdata` helper 只在固定短窗口内解码。

- `D32138` 在调用目标对象首 vtable slot 前，将本地 R9 辅助结构的首 qword 与 `+8` DWORD 清零，再把该结构地址作为 slot 0 的 R9。`TaskRunner` slot 0 `0x31F8CEE` 将该参数传给 `0x31F88EA`；其首个分支检查恰为 `qword [R9]==0 && dword [R9+8]==0`。因此从 `D32138` 发起的该 slot-0 路径绕过后续 `GetCurrentThreadId` 分支，进入 `0x31F9164` 组装 record 后调 `0x31F90C6`。
- `0x31F90C6` 对 `TaskQueue+0x10` 调 `0xD3DE50`；若 `[TaskQueue+0x18]` 非空，则把组装 record 交给 `0x31F927C`。TaskQueue ctor `0x31F8E2C` 经 `0x760220` 为 `+0x18` 分配并清零 0x30-byte storage；此处只按访问和写入形状称其为内部 queue-like storage，不指定 C++ 容器或运行时类名。
- `0x31F927C` 使用该 storage 的边界/容量相关字段选择或扩展缓冲区，经 `0x1B54AF8` 获取目标位置，再调用 `0x217DE40` 复制 task-shaped record，并递增 storage `+0x28` qword。机器码支持记录写入与计数更新；不单独证明被写记录的业务语义、之后何时消费或执行成功。
- 另一静态调用路径中，`0x31F9494` 读取其 owner `+0x28` storage 的首条记录，以该记录 `+0x40` 的 16-byte 值和输入记录调用谓词 `0xAC8446`；谓词命中时，它把 owner `+0x28` storage 的首记录交给 `0x31F927C`，目标 storage 是 owner `+0x30`，随后将 source storage 交给 `0x31F9520`。固定-hash 直接-call census 发现 `0x31F9494` 仅由 `0x31F8404` 两处直接调用，`0x31F9520` 仅由 `0x31F9494` 调用，`0x31F8404` 的唯一直接调用点是 `0x217DBF1`；这不覆盖间接调用。该 owner 的动态类、谓词含义、它与 `TaskQueue+0x18` storage 的实例关系以及这段迁移是否构成完整 drain 均未识别；不能将局部迁移称为 callback quiescence。
- 固定-hash Capstone follow-up `r2-taskqueue-callback-enqueue-followup-20261015.py/.txt` 保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\response-consumer-followup-20261014\`，SHA-256 分别为 `FDB448B46D4950F9A5ED15C8B6FD494A520D11FBD0BCDCFCF885E9315131CB6C` / `F739D86DC31641A58EC1111B2D7CEC4297D96F6EB055837C69A09074A16C8EAA`。脚本校验输入 image hash，导出 `D32138`、TaskRunner slot 0、空状态/TID 分支、队列 record 写入与所选转移方法。
- 固定-hash 直接-call census `r2-taskqueue-record-transfer-direct-xrefs-20261015.py/.txt` 同目录保存，SHA-256 分别为 `C13D2F615030F5E3A8EB95F07F9795968C19A825BB7D716DFC49F2908AA173A3` / `2E267564036C6B8400F1BB1DDB42C97AA6EABCACCE292817C8AF4CA6C11CCA7A`。扫描限于可执行节 rel32 `E8`，并用 Capstone 验证调用边界与 `.pdata` owner；未解析间接调用或映像外调用。

此结果把固定样本中的 callback state 路径继续接到一个 queue-like record writer，但不是运行时 callback consumer / 实际执行线程 / 返回投递状态的证明；manager final owner、TaskQueue worker/consumer owner、完整队列 drain/quiescence、stop 成功、R3 stable identity 与历史 AV 根因仍开放。未访问 QQ/SnowLuma、未调用 native、未发送消息或运行 Cargo tests；R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.41 TaskRunner → TaskQueue owner/TLS membership（2026-10-15，read-only runtime）

在用户授权的最后启动测试实例上，observer v2 两次只读采样（间隔 136.633 秒）均观察到全局对象 vptr 为固定 hash 的 `TaskRunner@TaskQueue`；`TaskRunner+0x8/+0x10` weak pair 的对象、control block、正 strong count 与 `TaskQueue+0x58` owner TID 双读一致。owner TID 在该进程线程快照内，且对应线程 TLS 指向同一个全局 TaskRunner；每次该全局对象仅命中一条 TLS 项。该现场映射与本合同 §§7.39–7.40 的静态字段归属一致，并撤销旧 observer 把全局 `TaskRunner+0x58` 误标为 TID 的 mismatch 解释。样本、精确数值、报告/observer 哈希及边界见[thread contract §26](qq-native-thread-contract.md#26-2026-10-15-taskrunner-taskqueue-ownertls-membership-read-only-runtime-samples)与 execution ledger。

两次远程内存双读只说明各自采样窗口内字段读值一致，不锁定对象寿命、不证明读间没有释放/复用，也不构成 callback 调用、TaskQueue record consumer/执行、manager final owner、stop/cancel/wait 成功、全部 drain/quiescence、R3 identity 或 AV 根因证据。只做进程内存/线程只读，不调用 QQ 函数或 callback、不写目标、不发消息、不打开聊天；R1–R3 门仍未闭合，R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.42 TaskQueue producer/consumer record rotation and manager handoff (2026-10-15, fixed-sample static)

继续复核冻结 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。Ghidra 12.1.4 与 Capstone 5.0.7 输出均由本机 helper 先校验该 hash；分析范围是固定映像中的 RTTI/vtable、`.pdata` 函数边界及所列指令窗口，不含现场调用。

- TaskRunner RTTI/vtable RVA `0x43B6088` 的 slot 0 是 `0x31F8CEE`。已由 §7.40 连接的 producer 路径在 `0x31F90C6` 持有 `TaskQueue+0x10` 锁；若 producer storage `TaskQueue+0x18` 非空，则调用 `0x31F927C` 写入一个 `0x58` 字节 task-shaped record，并递增该 storage 的 `+0x28` 计数。
- `0x31F93CC` 先读 consumer storage `TaskQueue+0x20` 的 `+0x28` 计数；为空时取得 `TaskQueue+0x10` 锁并检查 producer storage `TaskQueue+0x18`。`0x31F9606` 在 consumer storage 为空时，在同一锁下交换对象 `+0x18/+0x20` 的两个 qword（`movdqu`、`pshufd 0x4e`、`movdqu`），释放锁后重查 `+0x20`。若仍为空则写入空结果；否则按计数除以 `0x2e` 选块，以 `0x58` stride 复制一条记录，调用 `0x31F96E4` 移出/移除内部记录，再由 `0x31F9734` 转交给调用者输出并标记 ready。这连接了 producer/consumer 两个槽的轮转与单条 dequeue/handoff，不证明该记录在现场被消费或执行。
- `ThreadTaskManagerImpl` 次级 vtable RVA `0x4260488` 的 COL subobject offset 是 `+8`，slot 1=`0x217DBB6`。该方法对次级 `this+0x40` 调 `0x31F8404`，地址换算对应 complete-object `+0x48` queue vector；queue scan 调 `0x31F93CC` 判断可读项，再到 `0x31F9606` 取一条记录。manager 随后翻转次级-subobject `this+0xd8` byte，即 complete-object `+0xe0` selector；记录 ready 时调用 `0x217DDC8`、`0x1804228C6`，再将 manager complete-object 和结果交给 `0x217DC74`。后者遍历 complete-object `+0xd0` 的 watcher 链，从每个 node `+0x10` 取 listener 并调用其 vtable slot 1。listener 的动态 RTTI、slot 1 实现、其是否最终调用应用/native callback 及执行线程仍未识别。
- 这也更正 §7.40 当时对第二条 `+0x28→+0x30` record transfer owner 的保留：`0x31F8404` 把 manager vector 选出的 TaskQueue 传给 `0x31F9494`，它访问同一 TaskQueue 对象的 `+0x28/+0x30` storage pair。此为 TaskQueue 内另一条 predicate-gated record-transfer 路径；不等同上述 `+0x18/+0x20` consumer，也不证明队列完整 drain。
- 汇总脚本 `r2-taskqueue-producer-consumer-summary-20261015.py/.txt` SHA-256 `39DE27814009E9226EB1BBD0F05B2A150383C1FD9FAD59CECDF6BDAE957C46A6` / `6B650FF1E26FF2CA68C672386573FD794DFBA38DBC348318485EA81A5D50ADB9`；Ghidra 导出摘要 `ghidra-taskqueue-consumers/summary.txt` SHA-256 `2E7CAFC84952DF079837EB0E3049679F21DED77A41CD12D697B48C42D4E922D5`。关键导出 `fn_031f9606.asm`、`fn_031f96e4.c`、`fn_031f9734.c`、`fn_031f8404.c`、`fn_0217dbb6.asm`、`fn_0217dbb6.c`、`fn_0217dc74.c` 的 SHA-256 依次为 `7D34E148E874F1A00F203377D464FC0185EB5A644EE029FFBE8B4223A8F1E7BF`、`C442ACE2457E525A47835AB24A20DCE6F2DF3F87BCE2B853E66D4FE67885701B`、`9A4E3E6AB1403EA044E40E09375A68267E8285F067E4AD6EE387498FA390D99D`、`822711A30BBF60390E035C682760D7EC348EE28FFCFC48DB2F38EBB1C2750105`、`DC30A35BFA837CE2FBD9C5E11C37F763E2EBC98B7EE277BFEAD5CC079ED427BE`、`799AA32141E92FF2CBFA914F8BF9CBFA1901A2D3ED68CD6CEBA6D4CBA14F80BD`、`34AE3AF03121D675E2055E40A4EFAA70B42E54C09A56B32666FEE17BB212EFCE`。以上产物在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\response-consumer-followup-20261014\`。

静态路径仍未定位 watcher listener 的 RTTI/具体实现、运行时 consumer 调用/执行线程、manager final strong owner、所有 dispatcher 的 stop 与 wait 成功、完整 callback drain/quiescence；R3 stable identity 和历史 AV 原因也未闭合。无 QQ/SnowLuma/native/message 行为，R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.43 ThreadTaskManagerImpl watcher-list construction and registration writers (2026-10-15, fixed-sample static)

继续追 §7.42 的 `ThreadTaskManagerImpl+0xD0` watcher list。输入仍是冻结 `wrapper.node`，SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；仅使用 RTTI/vtable 与有效 `.pdata` 函数边界。

- `ThreadTaskManagerImpl` primary vtable RVA `0x4260448` 的 RTTI 为 `.?AVThreadTaskManagerImpl@internal@xpng@@`，slot 5=`0x217DAF6`、slot 6=`0x217DB38`。构造函数 `0x217D242` 将 complete-object `+0x48` 交给 `0x217D4A8`；helper 以子对象相对 `+0x80/+0x88/+0x90` 初始化，即 complete-object `+0xC8/+0xD0/+0xD8`：前两项都指向 `+0xC8` 的嵌入哨兵，计数清零。邻接的 complete-object `+0xE0` byte 设为 1，`+0xE8` qword 清零。由此补齐 §7.42 watcher list 的 self-linked empty initialization，不再把该初始化 writer 留作未知。
- slot 5 `0x217DAF6` 从哨兵的 tail（complete-object `+0xD0`）向后遍历，以 node `+0x10` 与传入 RDX 指针比较；已有同值时返回，否则调用 `0x606E52` 插入。该 helper 分配 `0x18` 字节 node，把 RDX 指向的值写入 node `+0x10`，在哨兵 head 插入并修复双向链接，递增 sentinel `+0x10` 计数。因此这里是一个嵌入式双向哨兵链；此结论限定于该固定布局，不另推断标准库容器类型。
- slot 6 `0x217DB38` 按相同 node `+0x10` 值查找，命中后调用 `0x217DB76` 解链；helper 修复相邻 node 链接、递减 list size，再调用 allocator 清理 node。与 slot 5 一样，局部函数体未见锁取得；调用方是否提供外部串行化仍未知。
- §7.42 的 ready-result visitor `0x217DC74` 从 complete-object `+0xD0` tail 走向 `+0xC8` 哨兵；每项 node `+0x10` 被作为 listener object pointer 取出，再调用其 vtable slot 1。构造、插入和移除现已静态闭合，但 ThreadTaskManagerImpl slot 5/6 的实际调用者、所注册 listener 的 RTTI/slot-1 实现、callback 的运行时执行与线程仍未绑定。宽泛的间接 slot-offset 搜索产生大量不同类型候选；未把仅凭 `[vtable+0x28]` 匹配的点认作此类注册者。
- 固定-hash复核脚本/报告 `r2-manager-watcher-list-construction-registration-20261015.py/.txt` SHA-256 分别为 `46777B57BCE7D53DC80F3CEF0BF9FBCC20C2D93C41347065B4EB5C8D328D1E88` / `E8928BD62D25A908E5ECAE466A3FE1496415F1E7A2B930E8588E70E4C63DC364`，保存在本机 `E:\stella\_reference\n4-r2-callback-manager-static-20261013\response-consumer-followup-20261014\`。脚本对固定 image hash、vtable RTTI/槽位、构造到 helper 的调用和上述函数指令逐条断言。另一次全映像 `[vtable+0x28]` 候选扫描计得 1,662 个 call site / 1,380 个函数；此偏移只代表 slot 5，不能用于类型或调用者归属。该扫描文本 hash 为 `3F5627B7D7B787FFFA3CA5F57D7E8F5545DF9A9344605CEC24FEDD912089DDD2`，仅作候选召回记录，不作正向绑定证据。

此项只闭合 watcher-list 初始化、slot-5 去重插入和 slot-6 移除的固定样本静态路径；不能证明任何节点在目标实例中登记/移除或 listener 被调用。manager last strong owner、dispatcher stop/wait 成功、全队列 callback drain/quiescence、R3 identity 与历史 AV 原因仍未闭合；R4 不准入，feat-005/006 blocked，产品 NOT READY。未访问 QQ/SnowLuma、未调用 native/callback、未写目标、未发消息或运行 Cargo tests。

### 7.44 Runtime watcher snapshot and MonitorTask slot-1 path (2026-10-15, read-only field evidence)

After explicit user authorization for read-only checks, a bounded observer followed only the fixed-image global TaskRunner anchor through its TaskQueue weak pair and the static TaskQueue+0x48 weak back-reference to ThreadTaskManagerImpl. The latest independent QQ root selected for the test account retained creation time 2026-10-15T00:59:34.989Z; its loaded module path was the frozen 9.9.33-52230 wrapper.node, disk SHA-256 63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0. Two snapshots 250 ms apart had the same process generation, module hash, TaskRunner/TaskQueue/manager weak pairs, manager vptr, watcher sentinel endpoints and size, and node/listener identities. Both traversals found exactly one node and returned to the sentinel with consistent forward/back links.

- The runtime manager vptr resolved to .?AVThreadTaskManagerImpl@internal@xpng@@. Its custom manager control block showed strong counter 0 and weak counter 2 in both passes. The fixed-hash constructor initializes this custom counter to zero and weak-lock helper 0x75ADB0 rejects only -1; therefore zero is eligible for a weak lock and is not treated as an expired sentinel. This observer did not increment the counter or pin object lifetime.
- The single node's +0x10 listener resolved at runtime and against fixed-image RTTI to .?AVMonitorTask@internal@xpng@@, vtable RVA 0x4260768. Its slot 1 is 0x31FA90E (.pdata [0x31FA90E,0x31FA96B)). That method checks this+0x108, acquires the lock associated with this+0x98, then traverses its own +0xF8/+0x100 list and calls each node+0x28 object's vtable slot 3. This identifies a locked fan-out path, not the inner callback types or a QQ business response. The inner list was not remotely read because the fixed slot-1 body acquires its lock before traversal.
- Fixed-image caller narrowing: manager setup 0x217D710 lazily creates a 0x110-byte MonitorTask via 0x217D7A3 → 0x217E26E, stores its shared pair at manager +0xE8, and calls 0x31FA36E with manager +0x48. 0x31FA36E obtains a TLS-selected object through 0x217D3F8 and passes the MonitorTask pointer through no-.pdata thunk 0x176CE8E, which dispatches the receiver's vtable slot 5 (+0x28). This is a concrete registration-caller candidate, but the TLS-selected receiver's dynamic RTTI is not bound to ThreadTaskManagerImpl, and this path is not proven to be the runtime node's registration event. The actual typed slot-5 caller remains open.
- The read-only observer requested process access mask 0x410 (QUERY_INFORMATION | VM_READ) and used ReadProcessMemory; it performed no target write, native invocation, callback invocation, chat action, or message send. SnowLuma was not accessed. A stable two-pass memory snapshot still does not prove lock ownership, object lifetime pinning, callback execution, consumer thread, stop/wait success, complete drain, or quiescence.

Ignored local evidence: local-evidence/n4-r2-readonly-20261015/watcher-chain-observer.ps1 / watcher-chain-20261015-103035.json SHA-256 C6139B3E126D144C656BAF8B8C6D46D2BF32D30AFFACFAEDF31DF9CB43E51193 / BDCA6B5F3196625DD35CE29FE194DF0ADF0B4A760C838F6ED9E790732B6A8651; monitor-task-slot1-static.py/.txt E7B04D4AC492A2B07111C65F82CED0A7EAB3B37D1CDDD39F125501C2664F555B / 5A0E80B742C5F533D947C31847510D973766CA743571EF3FA2B4EB100277C441; monitor-task-registration-candidate-20261015.py/.txt 41E7F1431163C23B3303C1158634BF71DC55042836DCDE01EF3ECF7E724A9283 / 48C68B4A2DC270D93239F2A7FE87A9B7EA98DA266342E2A3ADBE00D81E7CBF54. Raw target addresses remain in ignored local evidence.

This field sample narrows R2 to a runtime MonitorTask watcher and a registration-caller candidate; it does not establish that the monitored record is a native QQ response. The candidate receiver RTTI, callback semantics/execution/thread, manager final strong owner, successful stop/wait, complete callback drain/quiescence, R3 stable identity, and historical AV cause remain unresolved. R4 is not admitted; feat-005/006 remain blocked and the product remains NOT READY.

### 7.45 TLS-selected slot-5 receiver type binding (2026-10-15, fixed-sample static)

继续核对 §7.44 的 `0x31FA36E → 0x176CE8E` 注册候选，输入仍是 SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0` 的固定 `wrapper.node`。本节只绑定静态 TLS handoff 的预期类型，不声称该调用在现场执行。

- primary vtable RVA `0x4260448` 的 COL offset 为 0、RTTI 为 `.?AVThreadTaskManagerImpl@internal@xpng@@`；slot 2=`0x217D8CC`，slot 5=`0x217DAF6`。slot 2 保存入口 `this` 到 `RSI`，随后以 descriptor RVA `0x6781490` 调用通用 TLS setter `0x104C1C8`，第二参数是同一 `RSI`。descriptor accessor `0x217D40D` 的有界直接 caller 只有 `0x217D3F8` 与 primary slot 2 `0x217D8CC`；前者返回同一个 descriptor，转入 `0x104C1B8`。PE IAT 与两条 stub 机器码将 `0x104C18A/0x104C192` 分别绑定到 `KERNEL32!TlsSetValue/TlsGetValue`。
- `0x176CE7A` 调用 `0x217D3F8` 检查 TLS getter 非空；`0x176CE60` 再调用同一 getter，并把返回的原始指针写入一个局部 qword。`0x31FA36E` 依次调用这两个 helper，随后以该 qword 进入 `0x176CE8E`；该 thunk 解引用对象并通过 primary vptr `+0x28` dispatch。因此，当此 descriptor 由 `ThreadTaskManagerImpl` primary slot 2 写入时，slot-5 receiver 是该 `ThreadTaskManagerImpl` 对象，目标槽精确为 `0x217DAF6`。这不是 shared-pair pin，也不证明线程局部值仍有效或 setter/getter 之间无生命周期竞态。
- `0x217D710` 是创建 0x110-byte `MonitorTask` 并调用 `0x31FA36E` 的 setup 方法；有效 `.pdata`/Capstone 边界下，它只有一个直接 `E8` caller（`0xD31C19`）。这个静态路径与 §7.44 现场 `ThreadTaskManagerImpl` watcher 中的 `MonitorTask` 相容，且消除了“slot-5 receiver 类型完全未知”；它仍不能证明 `0xD31B7E` 运行时使用的 manager 就是被采样对象、该注册调用产生了现场 node、listener 已 callback，或 callback 所在线程。
- 固定-hash脚本与输出留在忽略目录 `local-evidence/n4-r2-readonly-20261015/`：`r2-manager-tls-receiver-type-20261015.py/.txt` SHA-256 `01C07A3402ED40D0553F47D2333E4F86E8953009CAFB73E9FBC1BF3F3A882482` / `6F319A3773432F8DFCD4FEA156C524BB637300AD191B9C203B518B023A3333E9`。脚本校验 image hash、COL/RTTI、slot 映射、TLS import、accessor 与 setter/getter 调用点、thunk 字节和有界 direct-call census；不读取当前 QQ 或 SnowLuma。

该结果将 TLS receiver 的 intended static type 收敛为 `ThreadTaskManagerImpl` primary slot-5 receiver，但不绑定当前 runtime registration event 或 listener child consumer。MonitorTask 内层 list 仍受 `+0x98` lock 保护，不远程读取。R2 consumer ABI/thread、manager final strong owner、stop/wait、完整 drain/quiescence、R3 stable identity 与历史 AV 根因仍开放；R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.46 Thread-start to conditional MonitorTask registration (2026-10-15, fixed-sample static)

在 §7.45 的 TLS receiver 类型绑定之上，继续向上追固定-hash `wrapper.node` 中唯一直接调用 MonitorTask setup `0x217D710` 的 thread-entry 路径。输入 SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；只解析 PE、RTTI/vtable、`.pdata` 函数边界与 Capstone 指令，不观察运行实例。

- `D31686` 于 `0xD316D4` 调 manager factory `0x217D16C`；factory 分配 `0xF0` 字节并在 `0x217D1B6` 调 `ThreadTaskManagerImpl` constructor `0x217D242`。有界 direct-call census 中该构造函数的唯一直接 caller 是该 factory。`D31686` 随后在 `0xD316E7` 将 factory 产生的 pair-shaped 值复制到 child `+0x110`，再从该字段取 manager 并于 `0xD316FB` 调 `0x217D58C` 配置它。
- `D31686` 的 `0xD31782` 将 `D31958` 作为 thread callback 参数传给 `D33418`，child 指针作为另一个参数；`D33418` 的间接 IAT call 经 PE import directory 精确解析到 `KERNEL32!CreateThread`。固定 bytes 中 `D31958` 是跳转到 `D31B7E` 的五字节 thunk。
- `D31B7E` 从 callback child `+0x110` 取回 manager，在 `0xD31BF4` 对该对象 primary vtable 的 `+0x10` 执行 slot 2；固定 RTTI/vtable 将此槽绑定到 `ThreadTaskManagerImpl::slot2` `0x217D8CC`，其 TLS setter 行为见 §7.45。随后 thread entry 检查 child `+0x100`：`0xD31BFE` 的 `JE` 在该 byte 为零时跳过 MonitorTask setup；非零分支于 `0xD31C07` 从同一 child `+0x110` 重新取 manager，并从 `0xD31C19` 调 `0x217D710`。该 setup 将 manager `+0x48` 交给 `0x31FA36E`，TLS-selected primary slot 5 路径见 §7.45。可识别的有效 `.pdata`/Capstone 边界下，`0x217D710` 唯一直接 `E8` caller 为 `0xD31C19`。
- 固定-hash脚本/报告 `r2-manager-thread-start-registration-20261015.py/.txt` SHA-256 分别为 `E8FE52B42809D6C41BD7EE1D8629F68BF5B8B8028083A2A7021545D66CEF3589` / `8CC51D6CFAD6C3AC32B68180218537129EFFDC37AB267A7DE3FC2E8F282E5C4C`，留在忽略的 `local-evidence/n4-r2-readonly-20261015/`。脚本校验精确 image hash、factory→constructor caller、RTTI/slot、holder field、callback thunk→entry、CreateThread IAT、thread-entry 指令、条件分支及 setup direct-call census。

这闭合的是固定映像中的 typed **conditional registration path**：只有 child `+0x100` 非零才走 MonitorTask setup。该 flag 的 writer/runtime value 尚未定位；静态路径不证明它在采样实例中为真，也不证明 setup 产生了 §7.44 的 watcher node。MonitorTask 内层 child list 仍受 `+0x98` lock 保护，未无锁读取。child node RTTI/slot-3 implementation/execution thread、现场 event-to-node 身份、manager final owner、stop/wait 成功与全量 drain/quiescence、R3 stable identity 和历史 AV 根因继续 unknown；R4 不准入、feat-005/006 blocked、产品 NOT READY。此次只读固定文件，未访问 QQ/SnowLuma 进程或执行 native/callback。

### 7.47 MonitorTask child collection dispatch and teardown (2026-10-15, fixed-sample static)

本节细化 §§7.44–7.46 所称的 MonitorTask “inner list”。输入仍是冻结的 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。分析只使用固定磁盘 PE、有效 `.pdata` 边界和有限原始可执行段窗口；未读取受 `+0x98` lock 保护的现场 child collection。

- MonitorTask primary vtable RVA `0x4260768` 只有本节可验证的三个入口：slot 0 `0x217E314`、slot 1 `0x31FA90E`、slot 2 `0x31FA96C`。因此 slot-3/slot-4 dispatch 指的是集合中 child object 的 vtable，而不是 MonitorTask 自身的 primary vtable。
- 构造函数 `0x217E26E` 安装该 MonitorTask vtable、初始化 `+0x98` 同步字段、清零 `+0xF0`，再把 `this+0xF8` 交给 `0x3BA6→0x3CEA`。helper 将头部首 qword 初始化为嵌入地址 `this+0x100`，并将头部另两 qword（`+0x100/+0x108`）清零。两个消费入口都以 `+0x108` 非零作为遍历门，并在进入集合前走 `+0x98` 同步 helper。
- slot 1 `0x31FA90E` 从头部 `+0xF8` 起、以嵌入 sentinel `this+0x100` 为终点，读取每个 node `+0x28` 的首 qword 作为 child object，并调用 child vtable `+0x18`（slot 3），将原始 `RDX` 透传给 callback。slot 2 `0x31FA96C` 使用同一头部、sentinel、lock path 和 `+0x28` 对象，但改调 child vtable `+0x20`（slot 4），并把 lock 子对象地址作为额外参数。
- 遍历推进 helper `0x45E5` 读取节点 `+0x8` 的子指针；无该分支时沿 `+0x10` 父指针上行并比较父节点首 qword。结合 `0x3FBAD0` 对节点首两 qword递归处理，可将结构限定为 **tree-shaped sentinel collection**；这仍不是对具体 STL 模板或 comparator 的识别。
- MonitorTask deleting destructor `0x217E314→0x31FA2A0` 进入清理。`0x31FA302` 在 TLS getter 成功分支调用 `0x176CE9C`，再将 `this+0xF8` 交给 `0x5CC71A`；该 helper 将根指针交给 `0x3FBAD0` 后，恢复 `+0xF8` 的 sentinel 关系并清零 `+0x100/+0x108`。递归 worker 对每个 node 的 `+0x28` 调 `0x19044→0x1EF48` pair-release 路径，然后经 `0x4C5C8` 按 `1×0x30` 请求 node storage release。该静态清理链不证明现场析构已发生、release 完整成功或异步 callback 已 quiesce。
- 头部初始化、slot-3/slot-4 消费和递归销毁现在已定位；**插入/注册写入者、child object RTTI、slot-3/slot-4 具体实现及执行线程仍 unknown**。`+0x98` 的现场内层结构未读，不能凭 watcher 外层节点 RTTI推断 child 类型。
- Hash-gated disassembly script/report 位于忽略的 `local-evidence/n4-r2-readonly-20261015/r2-monitor-task-inner-list-static-20261015.py/.txt`，SHA-256 分别为 `418A5C485AE6783B148EEC1EA85EE1D1617A9AA5780E346570D049704445BB4E` / `179E8B154D10827E327650D1CC76237B6D387D164E10A70D025CAB58D849FA09`。脚本检查精确 image hash、MonitorTask vtable 三槽、目标 `.pdata` 函数与 tree traversal/teardown 的有限指令；它不是整幅图的覆盖证明。另一个流式偏移筛选只对命中候选字节的函数做 Capstone 解码，不能闭合非直接或计算出的对象基址。

所以本项补齐的是 child collection 的固定映像初始化、两类调用入口和析构合同，不是运行时 child callback 身份/执行证明。flag `child+0x100` 的 writer/value、现场 watcher event-to-node 对应、manager final owner、成功 stop/wait、完整 callback drain/quiescence、R3 stable identity 与历史 AV 仍开放；R4 不准入，feat-005/006 blocked，产品 NOT READY。只读固定文件；未访问 QQ/SnowLuma 进程、调用 native/callback、写目标或发送消息。

### 7.48 Thread child +0x100 initialization and current watcher recheck (2026-10-15)

在 §7.46 的条件分支之后，继续将 `D31686` 的 `child+0x100` 与已知构造路径对齐。固定输入仍是 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；direct-call census 与函数解码通过 hash-gated 本机脚本完成。

- `D30FF0` 的有效 `.pdata` 范围是 `[0xD30FF0,0xD310D5)`；在 `0xD31081` 先将 `EAX` 清零，随后 `0xD31087` 写 byte `[RCX+0x100]=0`。它的四个可验证直接 `E8` caller 中，`0xCA94B0` 分配 `0x120` 字节对象、把同一 `RBX` 指针传给 `D30FF0`，再存入全局槽并从该槽重新取回传给 `D31686`。这建立了该调用路径中初始化对象与 thread-child receiver 的指针连续性，但没有 RTTI 类名。
- `D31686` 有 30 个经可执行节机器码及 `.pdata` 边界验证的直接 `E8` caller。函数本体把 manager pair 放在 child `+0x110`，设置 manager 后将 thread thunk 交给 `D33418`；其指令体没有直接编码的 child `+0x100` 字段访问。`D31B7E` 只在 manager slot 2/TLS seed 后读取该 byte，值为零时跳过 MonitorTask setup。对 `CA94B0→D30FF0→D31686→D31B7E` 这条可见路径，只确认了构造时清零和 entry 条件读取；没有确认任何同一对象的非零写入者，也不排除经别名、间接 helper 或另一设置调用发生写入。
- 单独排除了一个容易误并的偏移候选：`D31686` 的 30 个 call sites 来自 29 个 caller 函数，其中 `0x9C0C68` 是唯一带 non-stack `+0x100` 写指令命中的 caller；该指令在 `rsi=this` 时将 **外层 NTEngine `+0x100`** 的 16 bytes 清零。相同函数在 `0x9C0EE1` 传给 `D31686` 的 receiver 是从外层 `+0xE0` 保存地址再解引用取得的 child pointer，不是 NTEngine this。因此该写入不能归为 child `+0x100` flag writer。
- 因此，offset-wide `+0x100` 引用仍不能解释为 thread-child writer。其既有 13,774 个 memory operands / 2,697 个 first-operand write candidates 是跨对象召回清单；本次 targeted census 仅把上面同一指针的零初始化链与条件读取链相连，不据其它同偏移方法推断非零 writer。
- 在用户再次授权的指定测试实例上重跑外部只读 watcher observer。最新独立 QQ 根进程为 PID 27604，创建时间保持 `2026-10-15T00:59:34.989Z`，disk `wrapper.node` hash 与冻结值相同。双读均为 `ThreadTaskManagerImpl`，manager watcher size=1，唯一 node RTTI 为 `MonitorTask`；manager 自定义 strong count 两次均为 0，TaskQueue strong count 两次均为 1。该观察不 pin manager/object lifetime，也不证明它由上述 `CA94B0` 路径创建。采样仅跟随既有外层 watcher sentinel，没有读取受 `MonitorTask+0x98` lock 保护的内层集合。
- 证据只保存在忽略目录：`r2-child100-writer-callers-20261015.py/.txt` SHA-256 `31C376775FB3A883320F8477680E8007A387E4D0C330629774EDCDC9FB8F74D8` / `38381710FABCF5D9FC8861E177C7C2FE62BFAFD4146255909722E3FE53580091`；NTEngine receiver 差异脚本/报告 SHA-256 `D30956F9084899B972E0E6C718E89D55501F5A06B857C8D5C7886921711513E1` / `CB50ED773834D6F7E6704416504D04FF63EC3E2111B60A953252F0F756C37C6F`。新 observer report `watcher-chain-20261015-115229.json` SHA-256 `E9EDB00B60A514E4F41EFE3C9D1BED37C938E08348BB23399E8AF78ED781E76C`，observer 源 SHA-256 `C6139B3E126D144C656BAF8B8C6D46D2BF32D30AFFACFAEDF31DF9CB43E51193`。原始地址和完整 JSON 不入库。

- 后续窄化复核将 `CA94B0` 在 `D31686` 之后的固定直接调用 `D31A9A` / `D31AEE` 加入 hash-gated census。两个 callsite 都重新从 RIP-relative 全局槽装载 `RCX`；当前证据不能把该 receiver 归并为 `D31686` 的 child，也不能证明全局槽指针不别名。`D31A9A` 读取 receiver `+0x110`，条件调用 `D33302(receiver+0x30)`；`D31AEE` 在 receiver `+0x68` 调 `D332BA` / `D33302` 并读 `+0x60`。所选 helper 的可见直接内存访问限于这些 pair 子对象偏移，没有直接 `+0x100` 写指令；间接 helper/别名仍未覆盖。报告明确把无 `.pdata` 的 `D33B1E` 留作未解 target。driver/report SHA-256 `C09E46070AB270E91FFFBB80AB9DDD8EB6363AC862C47F72BF9FB860B58843E0` / `289302A70F20536600C7AA8AD617DC4BADEB0112832647CAC0D3E51E516676C0`；使用的 frozen-census source SHA-256 为 `31C376775FB3A883320F8477680E8007A387E4D0C330629774EDCDC9FB8F74D8`。

- T6 只读双读记录在 `2026-10-15T04:12:53Z`：最新独立 QQ 根 PID `27604`、创建代次 `1792025974989` ms，前后进程代次及磁盘 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0` 相同。两遍均为 `ThreadTaskManagerImpl`、一个外层 watcher 节点且 RTTI 为 `MonitorTask`；manager strong count 前后均 0、TaskQueue strong count 前后均 1。只持有 `0x410 (QUERY_INFORMATION | VM_READ)`，仅做 `ReadProcessMemory`；写入数、native 调用数、callback 调用数均为 0。report/stdout/observer source SHA-256 `B7EB7AFE37DB2A772A26414FB347EF8527C2873E3768A3DEB00F9913A2E8D260` / `648E34825A8505A70A00E3D651ABFD7479C6E5EF55E563870908CC2C08A7E324` / `C6139B3E126D144C656BAF8B8C6D46D2BF32D30AFFACFAEDF31DF9CB43E51193`。T6 仍不 pin 生命周期、不证明锁所有权、callback 执行或 node 来源；没有读取 `MonitorTask+0x98` 内层集合，SnowLuma 未访问。

这一步没有找到 thread child 的非零 flag writer，没有识别 MonitorTask 内层 tree 的 add/remove writer、child RTTI 或 slot-3/slot-4 实现，也没有闭合 consumer execution thread、manager final owner、stop/wait、完整 drain/quiescence、R3 identity 或历史 AV。动态样本仅外层只读观察；没有 QQ native/callback 调用、目标写入、聊天或发消息，SnowLuma 未访问。R4 不准入，feat-005/006 继续 blocked，产品 NOT READY。

### 7.50 MonitorTask inner-tree insertion-helper caller census (2026-10-15, fixed-sample static)

继续沿 §7.47 的 MonitorTask `+0x98` lock 与 `+0xF8/+0x100` tree sentinel 查 add/remove 写入者。输入为同一冻结 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；偏移扫描只作候选召回，直接调用边由有效 `.pdata` 与 Capstone 指令解码复核。

- 通用 helper `0x606E52` 会分配 `0x18` 字节 node，把调用参数中的 payload 放到 node `+0x10`，在传入 sentinel 前插入并修补相邻链指针，随后递增 sentinel `+0x10` 计数。它是固定样本中的通用 list-shaped insertion helper，不以此本身命名调用方对象类型。
- 对该 helper 的有界直接-call census 得到 43 个 rel32 `E8` call sites，分布在 39 个有效 `.pdata` caller 函数。RTTI 把已知 caller `0x217DAF6` 绑定为 `ThreadTaskManagerImpl` primary slot 5；该函数遍历并修改 manager 自己的 `+0xC8/+0xD0` watcher sentinel，不能归为 MonitorTask 的内层 `+0xF8/+0x100` collection writer。
- 39 个 caller 中有 8 个函数还包含 `+0x98/+0xF8/+0x100/+0x108` 候选偏移内存操作，其中 3 个出现非栈基址引用。`0x3935B90` 在同一寄存器基址上形成 `+0x98` 与 `+0x100` 地址，但未引用 `+0xF8`，并且函数访问同一候选对象的 `+0x210/+0x224`；没有 RTTI/指针别名证据把它绑定到 0x110-byte MonitorTask。其余命中是栈槽或单一偏移，不能证明对象身份。
- 因此本 census 没有找到可证明接收 MonitorTask inner-tree sentinel 的直接 `0x606E52` caller，也没有闭合该集合的 add/remove writer、child RTTI、slot-3/slot-4 implementation 或执行线程。它只限制这一 insertion-helper 的直接调用面；不覆盖间接调用、inline 链接代码、其它 insertion helper、计算/别名 receiver 或映像外 target，不能推广成“全映像无 writer”。offset-wide tree census 的 4,267 个选择函数仍仅是候选召回。
- Hash-gated local reports：`r2-monitor-list-helper-callers-20261015.py/.txt` SHA-256 `B5340FA7583427844E4B5BDC811B065D55AF44720944755BD760B0F8F149D602` / `D764CB97EDA315A307C79EFD92F0998C615C489FB9EE8D629CAC2EBDFB604383`；`r2-list-insert-callers-offset-filter-20261015.py/.txt` SHA-256 `F402BC544F188DF92CE1A5F892CD446185003F9DC611BABB98C44A6004221EE0` / `4E6104294784F815CFB052EC88128951A83E0E3DB6B9C0EF51ED923DEB045E37`。脚本和完整输出留在忽略的 `local-evidence/n4-r2-readonly-20261015/`；输入 wrapper hash 由脚本断言。

这一步只缩小了一个通用 insertion helper 的直接调用集合，未解除 MonitorTask inner-list 的锁边界。没有 QQ/SnowLuma 进程访问、native/callback 调用、目标写入、聊天/消息动作或 Cargo tests；R2 child identity/ABI/thread、manager final owner、stop/wait 与全量 drain/quiescence、R3 identity/历史 AV 均仍开放，R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.51 MonitorTask inner-tree direct argument and lock-helper caller screens (2026-10-15, fixed-sample static)

输入仍为冻结的 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；下列筛选只读取本机固定映像与已校验的 ignored census，不访问 QQ/SnowLuma 进程，不读取受 `+0x98` lock 保护的现场集合。

- 在 §7.50 的 43 个 `0x606E52` direct call sites / 39 个有效 `.pdata` callers 上追加限定参数 alias 跟踪：仅从 caller 入口 `RCX` 起始，跟踪简单寄存器 `MOV/LEA/ADD/SUB`，call 边界清除 volatile aliases。13 个调用点的 `RCX` 可归到入口对象偏移，均不指向 `+0x98/+0xF8/+0x100/+0x108`；其余 receiver alias unknown。已类型绑定的 `ThreadTaskManagerImpl` primary slot 5 caller `0x217DAF6` 使用 `RCX=this+0xC8`，与外层 watcher sentinel 一致。
- 对三处已识别的 MonitorTask `+0x98` lock helpers 作有效 `.pdata` 函数范围内 direct rel32 `E8` census：`0x31FAC18` 只有 caller `0x217E26E`（`MonitorTask` 构造路径，传 `this+0x98`）；`0x31FACB8` 只有 caller `0x31FA90E`（primary slot 1）；`0x31FAD12` 只有 caller `0x31FA96C`（primary slot 2）。这与构造初始化、slot 1/2 的同步访问路径相符；没有发现额外的该组 helper direct caller。
- 两个筛选都只限制已识别 helper 的直接 E8 caller 和简单 register aliases。没有覆盖 `RDX` 来源、memory-loaded/computed alias、间接/virtual calls、inline lock 操作、其它同步 helper、alternate insertion/removal helpers 或映像外 target；不能据此声称全映像没有 inner-tree writer，也不证明运行时锁所有权或 quiescence。
- 忽略目录脚本/报告：参数 screen SHA-256 `CBB8EC267231B2F36F8ABEDD255FA394F38066452261B389A7F3EB8F4715D5DA` / `9FB4F537267455272FA6846B92FC7F46A58AE6702700DE496A47428FA65F2EF4`；三 helper caller screen `CD9E1D850FBA73FF343D6ED7AC15123C9130B55CAC40F666AFF294EA4DD79D3A` / `1377B38CFF9C464E61D0398C51AF1509AD607298CB2F18B94571B1C016224AC6`。输入 helper report hash `D764CB97EDA315A307C79EFD92F0998C615C489FB9EE8D629CAC2EBDFB604383`；frozen census source hash `31C376775FB3A883320F8477680E8007A387E4D0C330629774EDCDC9FB8F74D8`。原始文件保存在忽略的 `local-evidence/n4-r2-readonly-20261015/`。

因此 §7.50 的 direct insertion-helper 路径与本节三处已知 lock-helper 的 direct call surface 均未识别内层 tree 写者。替代 helper、间接/inline 写入、child identity/ABI/thread、manager final owner、stop/wait/full drain/quiescence、R3 identity 与历史 AV 仍 open；R4 不准入，feat-005/006 blocked，产品 NOT READY。无 native/callback、目标写入、锁内现场读取、聊天/消息动作或 Cargo tests。

### 7.52 MonitorTask 条件树写入、处理器 RTTI 与虚表槽（2026-10-15，固定样本静态）

输入仍为冻结 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。本节闭合的是对象构造、条件树插入与固定映像虚表关系，不是现场 `MonitorTask` 内层列表快照。

- `0x217D710` 先进入注册 helper `0x31FA36E`，随后静态调用 `0x31FA3C6`；前一个 helper 是否完成外层注册取决于其 TLS receiver 分支。后一个调用的接收者是 owner `+0xE8` 中 MonitorTask pair 的首 qword。`0x31FA3C6` 以 MonitorTask `+0xF8` 为树对象，并按 `+0x18` 位字段 1、2、4 的独立条件准备键 1、2、4 的 key/value pair；每条路径分别经 `0x31FA69B`、`0x31FA759`、`0x31FA7D7` 构造 payload。
- 每个 factory 的构造函数直接写入经 RTTI/COL 验证的 primary vtable：键 1 为 `MonitorTaskProcessorFile@internal@xpng`（vtable RVA `0x45845C8`，constructor `0x3B34A7C`）；键 2 为 `MonitorTaskProcessorLog@internal@xpng`（`0x45846B8`，`0x3B35770`）；键 4 为 `MonitorTaskProcessorReport@internal@xpng`（`0x45847C8`，`0x3B3644C`）。
- 三条分支进入 `0x31FA70A → 0x31FAAAA → 0x31FAAC4` 的 tree find-or-insert 路径。未命中时 `0x31FAB5E` 分配 0x30-byte tree node，从 pair `+0` 取键并把 pair `+8` payload 转移到 node `+0x28`，再由 `0x42FC` 链接节点并增加 tree size。该 typed writer 解释了先前 `0x606E52` 与三处已知 `+0x98` lock-helper 阴性 caller screen 为什么没有命中内层树写入。
- `MonitorTask` primary slot 1 `0x31FA90E` 持 `+0x98` lock 遍历 tree，并对 node `+0x28` 对象调用 vtable slot 3；三个已构造 RTTI 类型的 slot 3 分别为 File=`0x28F0`（固定两字节 `ret; int3` stub）、Log=`0x3B357F4`、Report=`0x28F0`。primary slot 2 `0x31FA96C` 走同一受锁遍历，对对象调用 slot 4，并把当前上下文及 `this+0x98` 传入：File=`0x3B3524E`、Log=`0x3B35810`、Report=`0x3B365CA`。这些代码地址与类型已闭合；业务响应关联、现场 flag 值、内层节点成员及一次真实 slot 调用仍未验证。Log slot4 的 task-monitor/performance-log markers 有界见 §7.53。
- Hash-gated ignored artifacts：RTTI/constructor/vtable verifier `r2-monitortask-child-processor-rtti-20261015.py/.txt` SHA-256 `AFCF970BBFC0A1FB6677BB326391EE283757683685B29733D515B18B00507575` / `FBD4F9F2BBB63739DCA48A69F53AD03C9A2F5F6C9B6B55A6A97864A46D452BA0`；registration caller report `r2-monitortask-registration-followups-20261015.py/.txt` `9A01A8294F61EA7A30E6F21383FF47419098819747761C033056487908544678` / `666A1A75CB2F1611E7A41ED28303E7664AF777A1C181595C05B69569060D6DFC`；tree insert primitive report `r2-monitortask-tree-insert-primitives-20261015.py/.txt` `97C4D22CC90E8091E2380E3F759A0AB2DADA5C92569CAFDB64AFCD21C7AD662A` / `223F3B799C9497EB953905B2097F25C369571ABC8128E9ABFDCE630DDF8E73AB`。`217D710`/slot 1–2 context report `r2-monitortask-writer-consumer-context-20261015.py/.txt` hashes `8DC2839B1F0B7CD57282E7435B1D24039FA28A49683073D7E95E3D23E7C322FB` / `EE5C831B0B3031BB0253DCF74E75CF6579831EDCDCD7B7C6A43F3BEFEE964970`。完整材料仅位于忽略目录 `local-evidence/n4-r2-readonly-20261015/`。

本节将固定样本的 inner-tree writer、payload RTTI 与 slot 3/4 静态 target 从 unknown 收窄为上述条件构造路径；不证明 T7 外层 watcher 的 child tree 含这些节点，不证明这条注册路径在现场执行，也不将处理器关联到 QQ 响应事件或 consumer OS thread。slot 4 的部分内部用途有界见 §7.53。内层现场列表仍因 `+0x98` 锁边界而未读。manager final strong owner、stop/wait 成功、完整 drain/quiescence、R3 identity 与历史 AV 原因仍开放；R4 不准入，feat-005/006 blocked，产品 NOT READY。本轮无 QQ/SnowLuma 进程访问、native/callback 调用、目标写入、聊天/消息行为或 Cargo tests。

### 7.53 MonitorTask processor slot-4 field and sink screens (2026-10-15, fixed-sample static)

本节继续从 §7.52 的固定 vtable targets 向处理器字段和有界调用目标追查。所有内容仍绑定 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；只读 `.pdata` 函数与静态数据，没有 runtime callback invocation。

- `MonitorTaskProcessorLog::slot4` `0x3B35810` 中的固定字符串包括 `DidProcessTask`、`XpngMonitorTask`、源码路径 `..\\foundation\\xplatform-ng\\xpng\\task\\monitor_task\\monitor_task_processor_log.cc`、格式 `Thread_Name:{} Cost_Time:{}(ms) Cost_Cpu_Time:{}(ms) Hang_Time:{}(ms) Remain_Total_Task:{} Task_Entry:{}`，以及 `cost_time: {} < cpu_time: {}, it shouldn't happen`。函数将组装的值传给 `0x3B35D9E`；其已解码直接路径继续到 `0x3B360D6` 与 `0xD3A81F`。这些字符串和调用结构支持将此 Log 处理器限定为 task-monitor/performance-log 路径；不能将此类名或结构解释成响应 handler，也不证明现场该节点启用或执行。
- `MonitorTaskProcessorFile::slot4` `0x3B3524E` 检查对象 `+0x20` 配置指针，再以 `RDX` 上下文调用 `0xD3649E` 条件 helper；它从对象 `+0x28` 取得 16-byte 元素区间，按元素累计 `0x31F9050` 返回的长度，构造含时间格式常量 ` %Y-%m-%d% %H:%M:%E4S ` 的数据，并进入 `0x3B3556F`。该有限 body screen 没有解析最终文件/OS sink 或证明内容是什么。
- `MonitorTaskProcessorReport` constructor `0x3B3644C` 将两个 factory 输入经 `0x31C5F8` 初始化到对象子范围 `+0x30` 与 `+0x50`，另初始化 `+0x70`；所属 per-key factory `0x31FA7D7` 从 MonitorTask `+0x58/+0x78` 所指的输入对象取 `+0x10` 间接目标并调用，以构造传入值。其 slot4 `0x3B365CA` 分别对 `this+0x30`、`this+0x50` 调 `0x3FBAB2` 条件 helper，并在条件成立时通过对象内偏移 `+0x48`、`+0x68` 的间接目标调用。该 call target 来自对象内容，固定映像中没有绑定到具体接收模块或外部 sink；此处只确立两个配置驱动的 dispatch 位置。
- 本机 ignored hash-gated disassembly `r2-monitortask-processor-fields-20261015.py/.txt` SHA-256 `B7935ABA7A01E3B01E927F3228360003561F476B824AAFEFF00A601DFA8DEFDD` / `3389410769F897E3D4C670AF326427AE259ED2939ECE8E8711CDDEFA3EEFD3E6`；RIP-relative string 与 direct-call follow-up `r2-monitortask-processor-sink-refs-20261015.py/.txt` SHA-256 `EC680CB78AB1C84CBE67802CA3131FFC49CDAE4406D4344192BA89DC5E62992` / `3BF17E4CFE4F40EFF64F4CFD5ED074EA0D3398DBCD06CBF6427874CDDCEDD7CC`。脚本均先核对精确输入模块 hash，再用 `.pdata` 解码相关函数；证据仅在忽略目录 `local-evidence/n4-r2-readonly-20261015/`。

这将 Log slot4 的静态用途收敛到任务耗时/CPU 耗时/线程名观测格式；File 的最终 sink 和 Report 的两个间接目标仍未识别。三个类均未与响应事件、现场配置值、内层树的实际成员或执行线程绑定。没有 QQ/SnowLuma 进程访问、native/callback、受锁列表读取、目标写入、聊天/消息动作或 Cargo tests；R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.54 MonitorTask tree cleanup and known-lock direct-call boundary (2026-10-15, fixed-sample static)

继续核对 §7.47 的 MonitorTask 清理路径是否直接使用已识别的 `+0x98` lock helpers。输入仍是冻结 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；只读固定映像和既有 hash-gated `.pdata`/直接调用报告。

- deleting destructor `0x31FA2A0` 先调用状态清理 `0x31FA302`，再将 `this+0xF8` 交给 `0x3FBAC6`；后者把树根传给递归 worker `0x3FBAD0`。worker 递归处理 node `+0/+8` 子链接，对 node `+0x28` pair 走 `0x19044`，再经 `0x4C5C8` 请求按 `0x30` 大小释放 node。该指令链证明固定映像中的清理/释放调用形状，不证明运行时 destructor 已执行或释放成功。
- `0x31FA302` 只有在 TLS getter 成功分支才调用 `0x176CE9C` 和 `0x5CC71A`；后者从 `this+0xF8` 取根并调用 `0x3FBAD0`，随后复位头指针并清零 `+0x100/+0x108`。这条 TLS 条件清理路径本身不证明 callback 已停止或排空。
- 对照 §7.51 的有效 `.pdata` 直接 rel32 `E8` caller census，三个已知 MonitorTask lock helpers 的唯一直接 callers 分别为构造函数 `0x217E26E`、slot 1 `0x31FA90E`、slot 2 `0x31FA96C`。析构/清理函数 `0x31FA2A0`、`0x31FA302`、`0x5CC71A` 与递归 worker `0x3FBAD0` 没有进入这三个 helper 的直接调用集合。
- 结论只限已识别 helper 的直接调用：不能据此断言清理全程无同步，也不能排除 inline/indirect 或 alternate lock、TLS/owner 外层排他、并发回调或映像外协调；因此不构成 lock pairing、对象寿命、stop/wait 或 callback quiescence 证明。现场内层树仍未读。
- 固定映像 disassembly 脚本/报告 SHA-256 `418A5C485AE6783B148EEC1EA85EE1D1617A9AA5780E346570D049704445BB4E` / `179E8B154D10827E327650D1CC76237B6D387D164E10A70D025CAB58D849FA09`；已知 lock-helper caller census 脚本/报告 SHA-256 `CD9E1D850FBA73FF343D6ED7AC15123C9130B55CAC40F666AFF294EA4DD79D3A` / `1377B38CFF9C464E61D0398C51AF1509AD607298CB2F18B94571B1C016224AC6`。文件只在忽略的 `local-evidence/n4-r2-readonly-20261015/`。

因此 R2 teardown 的一个窄问题已由“未知是否直接使用这些已知锁 helper”收窄为“固定映像清理路径不直接调用这三者”；真正 teardown 的同步与寿命合同仍未闭合。没有访问 QQ/SnowLuma 进程、读取受锁现场集合、调用 native/callback、写目标或执行消息行为；R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.55 Report callable-pair construction and exceptional fallback (2026-10-15, fixed-sample static)

继续从 §7.53 的 Report slot-4 间接字段向上追到 `MonitorTask` 构造初始化、Report factory/constructor 与固定导入表。输入仍是 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；下述为固定映像静态结论，外层动态快照另记于 execution ledger。

- `MonitorTask` constructor `0x217E26E` 调 `0x9C11DA(this+0x18)`。该 helper 将静态代码地址写到 complete-object `+0x68/+0x70` 与 `+0x88/+0x90`；因此 Report factory 使用的输入记录 `MonitorTask+0x58/+0x78` 在构造后的初始 `+0x10/+0x18` 分别指向 RVA `0x2338/0x236A`。固定代码只证明初始值，不证明 factory 运行前未被其它写入替换。
- `0x31FA7D7` 对两条输入记录各取 `+0x10` 并以 `EDX=1`、`R9D=0x10` 间接调用。RVA `0x2338` 在该操作值下写出 `{0x2338,0x236A}` 两个代码指针到输出 pair。Report constructor `0x3B3644C` 再分别经 `0x31C5F8` 处理两个输入 pair，并把成员布置在 Report 子范围 `+0x30/+0x50`，其内部指针位置为 `+0x40/+0x48`、`+0x60/+0x68`。`0x31C5F8` 调来源记录 `+0x10` 时传 `EDX=0`，之后把来源记录的 `+0x10/+0x18` 写回 `0x2338/0x236A`；这里只描述传递与字节/槽位行为，不由此命名 C++ 类型或推断 factory 时的动态内容。
- Report slot 4 `0x3B365CA` 先分别对 `this+0x30`、`this+0x50` 调 `0x3FBAB2`。该 helper 进入 `0x3FCE5E`，取 pair 子对象首 qword 作为 virtual slot-0 目标，以操作值 4 调用并检查输出 qword 是否非零；`0x3FBAB2` 将该结果取反。slot 4 仅在对应条件成立时，才调用对象 `+0x48` 或 `+0x68` 的当前内容。故 dispatch 是否发生和最终地址都受实例状态控制；当前静态材料没有绑定后来可能写入的目标或下游 sink。
- 初始 invoker `0x236A` 是 thunk `0x236A→0x2374`。`0x2374` 构造栈上异常参数后进入 `0x3E6BB18`；其中 `0x4909B18` 是映像内槽，未落在普通/延迟导入表或 IAT 数据区，其 qword 指向本映像 `.pdata` thunk `0x3EB1DE0`（`jmp rax`），所以这一跳仍按对象内容间接分派。后续 `0x4910270` 经普通导入表解析为 `KERNEL32!RaiseException`；`0x49102D0` 解析为 `KERNEL32!RtlPcToFileHeader`。结合异常码 `0xE06D7363`，可确认该固定分支采用 MSVC C++ 异常机制；这不证明当前现场 pair 保持初始 invoker，也不证明 slot-4 条件命中或异常实际发生。
- 固定-hash bounded disassembly/import resolver `r2-report-target-input-upstream-20261015.py/.txt` SHA-256 `C2E3A3CF5FF47275BF6E6A33D0B9BDA154F759A4D24CE33AFD2BD437B679BAC9` / `026B8EEF0632D60B67156A72F01625C5A1806BD0887949CC430B399B4BB03049`；normal/delay import 与 slot resolver `r2-report-helper-imports-20261015.py/.txt` `C83A35E0F14C73E052B61F3574A12534E2B73DA0E76EAA3FFA39AA207BAD73BA` / `F0F9CFF43F0113F542DA0EBC2636ED61BA7B91A5A854AF9E0F1E9E1D94F2888C`。脚本均断言精确 wrapper hash；材料仅在忽略目录 `local-evidence/n4-r2-readonly-20261015/`。

因此 §7.53 所称两个对象内 target 已从“仅知来自对象内容”收窄为“构造初始 pair、条件查询和异常 fallback 有固定映像证据”，但运行时字段覆盖者、实际条件值、slot 调用和业务 sink 仍未知。新增 T8 只观察 manager 外层 watcher，不读 MonitorTask `+0x98` 锁内集合，不能将 `Report` payload 与现场节点关联。R2 teardown/执行门和 R3 identity/历史 AV 仍开放；R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.56 MonitorTask File processor terminal WriteFile sink (2026-10-15, fixed-sample static)

继续从 §7.53 的 File slot-4 `0x3B3524E` 追踪格式化内容的终端调用。输入仍是冻结 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；本节只描述该映像中的静态调用和导入绑定。

- `0x3B3524E` 构造临时字符串后调用 `0x3B3556F`，随即把临时字符串地址交给 `0x3B35666(this, string)`。`0x3B35666` 从 File processor `this+0x20` 取配置对象，按字符串表示选择 data pointer 与 byte length，再 tail-jump 到 `0xD36684`；前置 slot-4 路径要求该配置指针非空。
- `0xD36684` 将配置对象首 qword 作为 Win32 file handle，以临时字符串 data/length 作为 buffer/byte count，并把本地 DWORD 地址作为 bytes-written 输出指针；第五参数清零。固定导入表将间接 call slot RVA `0x4910470` 绑定为 `KERNEL32!WriteFile`。因此这个固定映像的 File slot-4 路径确实进入 `WriteFile`，而不只是把字符串保存到字段。
- `WriteFile` 返回零时，该 helper 转入 `0xD366E0` 并将其返回值写入配置对象 `+0x8`；字符数带符号位时则返回 `-1`。`0xD366E0` 的后续导入/错误语义没有在本节解读。File constructor `0x3B34A7C` 将 processor `+0x20` 初始化为 null；谁在构造后安装该配置对象/handle、输出文件路径和内容的业务含义、该输出是否与 QQ 响应相关均未确定。
- hash-gated bounded disassembly script/report `r2-file-processor-terminal-followup-20261015.py/.txt` SHA-256 `A159F8930147DE0715EE152A17B34897CDF4FD7737E27F506DE840C08F5CC5A5` / `5B9BF967031E57013E5550094F7EC775CD341A25BF95ECFA129CAC4C87A07364`；normal/delay import resolver `r2-file-terminal-import-resolve-20261015.py/.txt` SHA-256 `758DD6CA2F5B8803C1A06724E51A69A347439B002F110631C30EA8A8FDE511A6` / `582B109C6E13C746A24F750D72B959AB9AB9CD025E68D21E861882ABFA0C2D1F`。输入 hash 在两个静态脚本内断言；原始输出只在忽略的 `local-evidence/n4-r2-readonly-20261015/`。

该结果闭合的是 frozen File processor slot-4 的 Win32 sink，不证明现场 `+0x20` 已配置、File processor 节点已插入/被调用、写入成功、文件内容语义、OneBot/QQ 响应关联或执行线程。没有 QQ/SnowLuma 进程访问、native/callback 调用、目标写入、受锁现场读取、聊天/消息行为或 Cargo tests；R2 teardown/quiescence、R3 identity/历史 AV 仍开放，R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.57 File processor `+0x20` global offset-writer screen (2026-10-15, fixed-sample static)

为追查 §7.56 的配置/handle 来源，对同一冻结 `wrapper.node` 做了 `.pdata` 全函数指令操作数筛查；输入 SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。全量筛查覆盖 525,295 个有效函数与 18,889,451 条解码指令，记录 273,380 个位移 `0x20` 的内存操作数、164,599 个第一操作数写候选。

- 这是跨对象偏移召回清单，包含大量栈局部和无关类型，不能直接按 `+0x20` 归属 File processor。与已知 File 构造/注册路径交叉核对时，File 构造函数 `0x3B34A7C` 唯一对应字段访问是 `and qword ptr [rdi+0x20],0`，即构造时清零；已知注册工厂 `0x31FA69B` 构造对象后将指针交给 pair/tree 注册路径，没有对该字段的直接写入。其上层 `0x31FA3C6` 命中的 `[rsp+0x20]` 只是栈参数暂存，不是 File 对象字段。
- 已知 File processor vtable 的析构/消费路径和 slot 4 消费代码没有从本筛查显示出另一个可绑定的 `[File this+0x20]` 写入。全映像剩余候选尚未完成对象别名/接收者类型证明；计算出的地址、间接/虚调用、外部映像 helper 或通用配置对象写入均未排除。因此准确结论仍是“File `+0x20` 构造后 writer 未定位”，不能宣称全映像无 writer。
- ignored hash-gated 脚本/全量报告 `r2-file-plus20-global-writer-screen-20261015.py/.txt` SHA-256 `196DD2C46F25B56BD66ECF26B8FE780F220520A337C19073B102553FDCE94582` / `D69CD067A6C4D71BE77F04E619AD37EAA4C17C0CC43111B530E940C66A92ACE7`。脚本对精确输入 hash、PE32+ image base 和 `.pdata` 表做断言；报告保留跨对象 raw candidates，材料仅在忽略的 `local-evidence/n4-r2-readonly-20261015/`。

本节只增加固定映像静态筛查；无 QQ/SnowLuma 进程访问、native/callback、目标写入、受锁现场读取、聊天/消息动作或 Cargo tests。File 配置、WriteFile 成功、输出业务含义与响应关联、processor 现场成员/执行及 OS thread 仍未证；R2 teardown/quiescence、R3 identity/历史 AV 未闭合，R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.58 ThreadTaskManager child-owner replacement and teardown direct-call census (2026-10-15, fixed-sample static)

继续从 child `+0x110` holder-release 路径追 manager 最后 owner 与 stop/wait 边界。输入仍是冻结 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；扫描解析可执行节中的 rel32 `E8`，并要求 Capstone 指令边界与有效 `.pdata` caller 同时吻合。

- 在 525,295 个有效 `.pdata` 函数中，manager dtor `0x217D4FA` 只有一个直接 caller `0x217DD98`；deleting-destructor thunk `0x217DD98` 没有 direct `E8` caller，仍由既有 RTTI/vtable 证据确认。factory `0x217D16C` 的唯一直接 caller 是 `D31686`，构造 `0x217D242` 的唯一直接 caller 是 factory。这个结果仅限制固定映像直接边，不覆盖 shared-control-block 间接析构、虚调用、inline/外部映像 owner 路径。
- child dtor `D31356` 有 3 个 direct `E8` sites：通用 owner helper `0x4E91BE` 一处，以及 `0x1B63EC0`、`0x1B643C0` 两处 MSFThread 路径。stop helper `D313D6` 有 18 个 sites / 12 个 `.pdata` caller functions；wait/close helper `D3355C` 有 2 个 direct callers（`D313D6` 与 `0xC71F9A`）。这些数目与已记录的 owner/stop 路径相符，没有新增可绑定为 manager final strong owner 的直接边。
- `0x4E91A6` 自身没有 `.pdata` entry；其固定短窗口先以 `RDX` 替换 `[RCX]`，若旧值非空则 tail-jump 到 `0x4E91BE`，后者对旧 child 调 `D31356` 再 tail-jump 到 `0xB750029`。对 `0x4E91A6` 的 `.pdata`-bounded direct-call census 得到 41 个 sites / 29 个 caller functions；其中 24 个 caller functions 同时直接调用 `D31686`。这是 function-level overlap，不能单独证明两条边使用同一字段或同一对象。简单线性 alias screen 对其中 19 个函数标记相同 `arg0+offset` 形状；它不做 CFG/path merge，因此仍是候选筛选，不作对象类型或实际 owner 证明。
- 对全部 29 个 `D31686` caller functions 做严格 `.rdata` RTTI/vtable membership screen（校验 15,410 个 vtable starts、next-header 边界及表项 executable range）后，3 个 caller 被直接映射到已知 slot 2：`0x1B63FC0`=`MSFThread_xpng`，`0x9C4852`=`FileDragManagerWin`，`0x290C746`=`ThreadPoolImpl`。24 个 replacement-helper overlap callers 中只有 `0x9C4852` 有直接 vtable entry；其 `this+0xA8` setup/teardown 已由 §7.30 独立绑定。其余 overlap caller 没有直接 `.rdata` vtable membership 命中不排除非虚方法、间接调用、thunk 或未识别 owner，也不证明同字段或 runtime identity。
- 新的精确入口核验发现 `0xB81758` 仅有两个经 executable-byte `E8` 搜索、Capstone 指令边界及 `.pdata` owner 三重验证的直接 caller：`MSFService@nt` primary vtable slot 16 (`0x72EC24`) 与 `MSFCoreService@nt` primary vtable slot 16 (`0x742DDA`)。两者都按 `call 0xB816E8; mov rcx,rax; call 0xB81758` 传递 getter 返回值。`0xB816E8` 返回 guard 相关的全局状态 RVA `0x67510A0`；无 `.pdata` init stub `0xB81748` 清零覆盖 0x18 bytes 的状态区域。`0xB81758` 通过 `0x4E91A6` 替换 state+0，随后从 state+0 取 child 调 `D31686`（后者在 child+0x110 建 manager holder），成功分支把 `D31AC0` 结果 move-assign 到 state+8，失败分支将 state+0 清空。全 525,295 个有效 `.pdata` 函数 / 18,889,451 条指令的 RIP-relative operand scan 在 state/guard 范围命中 6 处，均在 getter/accessor；该扫描不覆盖无 `.pdata` code。由此把 process-global state+0 识别为 manager 生命周期相关的静态 child-owner 候选，并将 setup helper 的直接调用面收窄到两个 typed entry；尚未定位谁最终清除此字段/销毁全局状态，也没有证明最后 strong owner、析构发生或该路径在现场被执行。
- 既有 child teardown 是有条件的：child 最终进入 `D31356` 时先到 `D313D6`，再释放 child `+0x110` manager holder。`D313D6` 选取 manager index-0 `TaskRunner` 路径，并对 child `+0x10` HANDLE 走 INFINITE wait/close；返回值未检查。上述 direct-call census 不证明运行时 last-owner、析构触发、wait 成功、其它 manager entries 均已停止或 callback queue 全局 drain/quiescence。
- hash-gated ignored artifacts：全局状态 owner-chain verifier/report `r2-manager-global-state-owner-chain-20261015.py/.txt` SHA-256 `319670C6F9A3E009074BA30200A0746A42C319553212C7DB7D734D493290FD4C` / `FA49A7D55B229EA35DD1F5675870BD66C01550D15BCC7140BBCCEB7992F9B78A`；其它 direct-call、overlap、helper-window、linear-alias 与 RTTI/vtable 脚本/报告 hash 见前述条目。原始材料仅在忽略的 `local-evidence/n4-r2-readonly-20261015/`。

本节只收窄 frozen image 的直接 caller、raw child replacement 及 global state child-owner candidate；不把候选提升为已证实 final strong owner 或动态寿命事实。没有 QQ/SnowLuma 进程访问、native/callback 调用、目标写入、受锁内列表读取、聊天/消息动作或 Cargo tests。manager state teardown/finalizer、最后 strong owner、成功 stop/wait、全项 stop、完整 callback drain/quiescence、consumer ABI/thread、R3 identity 与历史 AV 仍开放；R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.59 Guarded global state finalizer and module-detach screens (2026-10-15, fixed-sample static)

继续检查 §7.58 的 global child-owner 候选 `0x67510A0` 是否有静态 finalizer、module detach 或 TLS 清理路径。输入仍是精确 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；本轮只读本机映像，不访问 QQ/SnowLuma 进程。

- 对 `0xB816E8` 的 29 个有效直接 `E8` 调用点（分布在 25 个 `.pdata` caller functions）做返回值前序筛查：24 个窗口仅读 global `+8/+0x10` pair 字段；9 个 direct calls 到无 `.pdata` helper `0xB81944`，其短体仅比较 `[RCX+8]` 是否非零并返回布尔值；没有窗口对 global 字段直接写入。安装 helper `0xB81758` 仍只有两个已绑定的 primary slot-16 调用点（§7.58）。这是一段线性、首个后续 call/ret 截止的筛查，不覆盖 caller 之后的间接副作用。
- `.pdata` 全量 RIP-relative operand 结果复核沿用 §7.58 hash-gated report（525,295 functions / 18,889,451 instructions / state+guard 范围 6 refs，均位于 getter）。新 verifier 重新核验了该报告 SHA-256 `FA49A7D55B229EA35DD1F5675870BD66C01550D15BCC7140BBCCEB7992F9B78A`，并重新执行 direct `E8` / TLS / PE 入口检查；未重复全量解码。
- PE TLS directory 列出回调 RVA `0x3E68820` 与 `0x3E67F70`。后者在 detach 分支遍历 TLS 局部回调表并通过间接调用执行；两个 callback 都没有 direct `E8` 到 `0xB816E8/0xB81758`。回调表的运行时内容和间接目标未知，不能由直接-call 缺失排除它们最终触发外部清理。
- DLL `AddressOfEntryPoint=0x3E6B154` tail-dispatch 到 CRT entry dispatcher `0x3E6B02C`；其 process-detach 分支进入 CRT uninitialization helper，DLL-specific stub `0x5C7470` 仅返回成功。已导入符号中没有 `atexit/onexit/cxa_atexit/initterm` 名称，且 state/guard 的绝对 VA/RVA qword literal 搜索为零；静态链接 CRT、相对地址、间接注册和运行期 callback table 不受此结果排除。getter 的 guard/header/init/footer 路径与零化 init stub内没有显式用户 finalizer 注册调用。
- ignored fixed-hash verifier/report `r2-global-state-lifecycle-followup-20261015.py/.txt` SHA-256 `BA7756F393D76302C527DD46165A022657ECDD05A0CAF4E8CD762ED57ACD9057` / `C44FBD00D9619C4C9F43CB8F02F969486020A5511A654EBA253C4F348830E488`。原始材料只留在 `local-evidence/n4-r2-readonly-20261015/`。

结论只到“本轮检查的 global getter/setup 直接边、全量 `.pdata` RIP refs、TLS callback 直接边和 DLL detach 入口中未识别出 global-state finalizer”。CRT/线程清理仍包含间接回调表，no-hit 不能证明不存在动态注册或外部 owner；global state 最终清除、manager last strong owner、successful wait、全局 stop/drain/quiescence 仍未知。无 QQ/SnowLuma 进程访问、native/callback 调用、目标写入、锁内列表读取、聊天/消息动作或 Cargo tests；R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.60 CRT detach static termination-table screen (2026-10-15, fixed-sample static)

继续 §7.59 的 CRT process-detach/onexit 路径检查。输入仍是冻结 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；只解码磁盘映像，不访问 QQ/SnowLuma 进程或执行映像代码。

- 精确入口 `0x3E6B154` dispatch 到 `0x3E6B02C`；DLL detach reason 0 从 `0x3E6AFDC` tail-dispatch 到 CRT uninitializer `0x3E6AF58`，在返回成功的 DLL-specific stub 后继续。detach helper 中 `0x3E6BA60` 通过 IAT 调 `KERNEL32!InitializeSListHead` 初始化 BSS head `0x6D8E5C0`；`0x3E6BA70` 到 `0x3E6D4F4`，其 bounded body 调 `KERNEL32!InterlockedFlushSList` 并释放链表节点。这条 SList drain 不能当作 onexit callback 执行证据。
- 已定位另一条固定终止数组：`0x3E6E4DC` 在 `reserved==NULL` 分支把 `[0x45BAF80,0x45BB080)` 交给 `0x3EA2F00`。执行器从尾部反向按 16-byte record 读取第二个 qword，对非空项经 CFG helper `0x3EB1E20` dispatch。数组共 16 项，13 个非空值、10 个唯一目标。只对这 10 个目标作有界 decode；没有一项直接调用 getter `0xB816E8`/setup `0xB81758` 或对 `0x67510A0` 状态/guard 区作 RIP-relative 引用。这个结论只约束该固定数组及其 bounded callback bytes。
- 复用 §7.58 全 `.pdata` RIP census（先核验原 report SHA-256 `FA49A7D55B229EA35DD1F5675870BD66C01550D15BCC7140BBCCEB7992F9B78A`，不重复 18,889,451 条解码）；重新验证 getter 为 29 个直接 sites / 25 个 callers，且固定终止表 callback 与 getter caller 集合无交集。Import 表仍无 `atexit/onexit/initterm` 名称，但这不排除静态链接、间接注册或运行时 callback table。
- hash-gated verifier/report `r2-crt-detach-static-exit-screen-20261015.py/.txt` SHA-256 `7852C7E8CE6C27C7C8115094EBF785D00B1BEBFF8BF08831A6E5F3D5A2DC3FD6` / `F8C14BAE8866CB36387620A0B9493624CE35C414F26C50FA00B829619AEE80DB`；原始材料仅在 ignored `local-evidence/n4-r2-readonly-20261015/`。
- Microsoft 文档说明 DLL 的 `_onexit` 函数在卸载时于 `DllMain(DLL_PROCESS_DETACH)` 后运行，另说明 onexit table 的成员布局是 opaque CRT 实现细节；因此这里仅称为固定映像 termination array，不把观察到的 record layout 扩展命名成通用 onexit ABI（[Microsoft `_onexit`](https://learn.microsoft.com/en-us/cpp/c-runtime-library/reference/onexit-onexit-m?view=msvc-170)，[Microsoft onexit-table routines](https://learn.microsoft.com/en-us/cpp/c-runtime-library/execute-onexit-table-initialize-onexit-table-register-onexit-function?view=msvc-170)）。

本屏没有找到能清除 guarded manager-owner state 的固定数组回调，也没有定位动态 `_register_onexit_function` writer/table 内容。间接 callback registry、no-`.pdata` alias、external owner、manager last strong owner、stop/wait 成功与全局 drain/quiescence 仍未知；不得宣称不存在 finalizer。无 QQ/SnowLuma 进程访问、native/callback 调用、目标写入、锁内列表读取、消息动作或 Cargo tests；R4 不准入、feat-005/006 blocked、产品 NOT READY。

### 7.61 TLS thread-exit callback-list writer and caller screen (2026-10-15, fixed-sample static)

沿 §7.59–§7.60 未闭合的线程 detach 间接 callback list，进一步检查冻结 `wrapper.node` 的 TLS cleanup list。输入 SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；本节只解码磁盘映像，不访问 QQ/SnowLuma 进程或执行其中代码。

- TLS callback `0x3E67F70` 从同一 TLS index RVA `0x6755690` 取得线程块，读取块内 `+0xB0` 的列表头；它反向遍历 block 中的 callback pointers，执行非空项，再释放该 block 并继续其前驱。`0x3E68018` 是对应的线程局部注册 writer：从同一 index/`+0xB0` 取得当前 block，计数到 `0x1E` 时分配 `0x100` bytes 的新 block、链接旧 block、在 callback array `+0x10` 写入传入的 RCX 函数指针并增加计数。此形状标记为该映像的 TLS thread-exit list，不命名为进程级 `_register_onexit_function` ABI。
- executable-section rel32 `E8/E9` 召回并校验指令边界与 `.pdata` owner 后，`0x3E68018` 有且仅有 4 个已验证直接 E8 caller：`0xB0D31→0xB0DBA`、`0xB8943F→0xB894AE`、`0x219494F→0x2194956`、`0x2DE5070→0x2DE507A`。4 个实参分别沿固定 TLS state cleanups、TLS `+0x50` release、通用 context/cleanup pair、TLS `+0xD68` object release；直接调用面不证明所有间接/inline 注册点都不存在。未找到对 exit callback `0x3E67F70` 的直接 E8/E9 reference；thread-init callback `0x3E68820` 有一个 out-of-`.pdata` E9 候选 `0x3E68891`，没有验证为指令边界/owner，因此不计为已证 call edge。
- 通用 pair helper `0x21948DC` 的唯一有效直接 E8 caller 为 `0x219547C`：传入 context `0x219548A` 返回的对象，并把 cleanup entry `0x219551A` 存入 TLS pair；TLS callback thunk `0x2194956` 最终经 `0x6BFCCB2` 先清空 context slot 再间接调用 pair 的 cleanup。cleanup body 对 guarded global manager state `0x67510A0..0x67510BF` 无直接 RIP-relative reference，但会释放对象 `+0x38` 并继续到 `0x18739805A` 的动态 control-block release；实际 control-block type/value 与其最终析构目标未知。
- `0x2DE4EB1` caller 在该 TLS 块 `+0xD68` 初始化 `0x9C8`-byte state（含 `0x270` 个 32-bit recurrence slots 和常量 `0x6C078965`），并注册 `0x2DE507A` 清理该字段及 object release。它不能绑定为 ThreadTaskManager 或 guarded global-state owner。
- Hash-gated verifier/report `local-evidence/n4-r2-readonly-20261015/r2-tls-exit-list-registration-followup-20261015.py/.txt` SHA-256 `27F876AF9C64BFE4BFC77119CEF6D05BDC73FA41229C28F4255454FF734BCEDA` / `957A8D2FB381389FABD4A086CFBB4151D83B4FFEBAF707FB9A8785C0CE8F1E2C`。报告只记固定样本的有界静态结果；未做 runtime TLS list 读取、native/callback 调用或 unload 实验。

本节定位了 TLS cleanup list 的 writer 及其有限直接调用者，但没有定位 generic process `_register_onexit_function` writer/table，也没有证明 TLS 回调实际注册或执行。manager final owner、dynamic control-block release、no-`.pdata` 间接别名、stop/wait 成功、全局 callback drain/quiescence 继续 open；unknown 不代表不存在 finalizer。无 QQ/SnowLuma 进程访问、native/callback 调用、目标写入、锁内列表读取、聊天/消息动作或 Cargo tests；R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.62 Loaded `.text` delta and process-onexit allocator alias screen (2026-10-15, read-only runtime plus fixed-sample static)

在继续冻结映像的 CRT/TLS cleanup 筛查时，对用户指定的最新 QQ 根进程做了一次只读模块代码比对，并补查 `HeapReAlloc` 入口周围的无 `.pdata` alias。磁盘输入仍为 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。

- 2026-10-15 18:39:04、18:39:58 与 18:43:41（+08）选中的根 PID `27604`、创建时间 `2026-10-15T00:59:34.9891069Z`；加载模块路径、磁盘 SHA 与冻结样本匹配。reader 仅用 `OpenProcess(0x410 = QUERY_INFORMATION | VM_READ)` 读取 PE 头标识和映像 `.text`，未读数据对象、MonitorTask 内锁列表或聊天数据；未写目标、调用 native/callback 或执行消息行为。PE32+ AMD64 头身份字段与磁盘匹配，模块基址 `0x7FFA096F0000`。
- `.text` 为 RVA `0x1000`、虚拟长度 `0x3EB630D`。disk-normalized SHA-256 为 `3CFD65B02D78F4097A6441A53C829D50511BC51D48C8025CE608F3839D731B7B`，loaded `.text` SHA-256 为 `DF44C4F7DC935BF69658DF40CC2BEFA9C2F0E6B09FB32697FA49990B82DE302B`；三次观测均只有 RVA `0x3E91B50–0x3E91B51` 两字节不同，磁盘为 `48 83`，进程内为 `CC C3`。固定 `.pdata` 将该地址映射到 `[0x3E91B50,0x3E91BA6)`；磁盘函数余下路径含条件 fast-fail/exception/exit 分支并以 `INT3` 收尾。精确指令边界扫描得到该入口 114 个已验证直接 `E8` sites、分布于 101 个 `.pdata` callers。
- 这证明样本时点的内存 `.text` 不与磁盘文件完全一致；没有归因补丁来源、是否有意、运行时是否进入该函数，或它是否解释历史 AV。两个改写字节位于一个终止/异常路径入口，不能作为 manager finalizer 或 quiescence 的证据。磁盘 hash 与加载文件 hash 必须和当前内存 `.text` 分开描述。
- 补查 `HeapReAlloc` IAT `0x49100B8` 的固定映像 caller：其 body `0x3EA49F0` 前有无 `.pdata` thunk `0x3EA49E0`（`jmp 0x3EA49F0`）；通过 thunk 的 8 个直接 caller sites，经指令边界与 `.pdata` 验证，分布在 `0x3E7EA20`、`0x3E7EC80`、`0x3E7EFA0`、`0x3E7F180`、`0x3E7F5F0`、`0x3E7F860`、`0x3E7FB70`、`0x3E7FD50`。有界窗口呈现通用 reallocation/copy/error paths，未将其绑定到 process onexit table。另一个 HeapReAlloc wrapper `0x3EA9040` 由 `0x3E8B870` 的一个直接 call site 使用；该 helper 的 7 个直接调用点位于 4 个 `.pdata` functions，呈现 count/element-size buffer growth。注意，仅按 `0x3EA49F0` body entry 搜索会漏掉前述 alias caller；follow-up 已补正该召回边界。
- 对应 ignored artifacts：loaded-text reader/report `loaded-text-verify-20261015.ps1/.json` SHA-256 `C9B2EC1D67E5898CC42F5DA90B2F11F9F624065D286A24247C12B9BAFCDD18F4` / `D053C4D0D22C9533ABC1521E860A45962E7E8B43BEA0C0E8B7BC93BD0D3C3476`；entry static binder/report `loaded-entry-static-bind-20261015.py/.txt` `3D7E6AE2A5D87732545E433E0054E82B12A9ECDF7EF548B19469B7CA4422E9BD` / `830831143038F2F66F3201C117728AA182D0962FE983F4B7B1A11DE9AF762CD9`；allocator/thunk verifier family `r2-crt-onexit-realloc-reference-screen-20261015.py` `71BAFD7F8023B77FD98A2A0F7DEFEEBE3F700CE4EC2D118DFE5E21861B1A3295`, `r2-crt-onexit-out-of-pdata-ref-screen-20261015.py` `A8A23E309D8371E1E052BA1E55C5B14D618C83E351C3F4B454469E6A78836E54`, and alias caller `.py/.txt` `1D1AC84E2DBF643BB2B281DB49AA9257A19D4FDBD66CA6C2BC9F05A7C8FB42F1` / `538A9FD96729046BB20DB735F0DD7454705154FCE9DA5FFDAF0059442B54C637`. Combined bounded summary SHA-256 `56F44E4830A25E5B59FCF224B66175650F97ACD846624FD4E933EE899F335289`; all files remain ignored under `local-evidence/n4-r2-readonly-20261015/`.

此项修正了 body-entry-only allocator search 的召回边界，但仍未定位 generic process `_register_onexit_function` writer/table 或运行时 callback registry 内容。no-hit 不排除其他间接/非 `.pdata` 路径。manager final owner、成功 wait、stop/drain/quiescence、consumer ABI/thread、R3 identity 和历史 AV 根因仍 open；R4 不准入，feat-005/006 blocked，产品 NOT READY。此样本为只读现场代码采样，不是 QQ 消息回程或业务 callback 验收。


### 7.63 TLS context-pair cleanup and control-block count path (2026-10-15, fixed-sample static)

对 §7.61 的 TLS context/cleanup pair 继续向下跟踪，输入仍为冻结 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；这次只解码固定映像，没有访问进程、执行代码或进行消息操作。

- 先校正 RVA：映像基址为 `0x180000000`，故 cleanup 中绝对目标 `0x182194B46` 对应 RVA `0x2194B46`，不是 `0x194B46`。hash-gated 重解确认 TLS cleanup `0x219551A` 读取 context `+0x38`，非空时调用 `0x739805A`；随后清除 TLS `+0xCF8`，在 static lock `0x6781770` 下把 context `+0xD8` 链接到 BSS free-list head `0x6781768`。acquire helper `0x219548A` 在同一锁下弹出该 free-list；无节点时申请 `0x1DF` bytes，将返回地址向上对齐 `0x100` 后初始化 context。构造体清零 `+0x38` 与 `+0xD8`，同时初始化 `+0xC8/+0xCC/+0xD0` 同步相关字段。由此确认的是 TLS context 池的静态复用形状，不等于 manager 对象寿命。
- `0x739805A` 从传入指针前 `0x10` 取控制结构，在已识别同步 helper 包围下调用 `0x2194B46` 校验编码头与 control pointer 配对，再检查并递减 control `+0x120` 的 32-bit 计数。插入路径 `0x2194C69` 建立对应 entry 后递增同一字段；`0x2194B46` 校验 `([p-0x18] xor [p-0x20]) == 0x4C833E95` 且 `[p-0x10] == control`。release 路径在旧值为 1 时允许减到 0，随后解锁返回；这个有界函数体内没有显式删除/析构调用，但其它 removal、indirect 或外部 owner 路径未排除，不能据此认定全局 final release 行为。control block 的业务类型、`+0x38` 的运行期值和谁写入该字段仍未知。
- 全映像 `.pdata` 单遍 operand census 覆盖 525,295 functions / 18,889,451 decoded instructions。TLS index RVA `0x6755690` 有 1,574 个 RIP-relative refs / 1,501 functions；它是广泛共享索引，单独不能归属目标对象。free-list head RVA `0x6781768` 仅命中 acquire/recycle 两函数内 4 refs；lock RVA `0x6781770` 命中这两函数内 2 refs。`0x2195465` acquire-and-register wrapper 在该 `.pdata` 扫描中没有 E8/E9 或 immediate inbound operand，且在所有映像节里未找到其绝对 VA qword；这是静态 inbound recall，不排除动态/外部/编码引用，也不证明上述 TLS callback 已注册或执行。
- ignored hash-gated body verifier/report `r2-tls-pair-control-block-followup-20261015.py/.txt` SHA-256 `9D23176D88BA5FBF281DE60BF755A8B04EF4FF8BADCE28476C8FB517AF0F4848` / `EF7AD374ECE433BEC9DF56085E67FF74A07CE34261619B0CA702F3D1086F56EB`；full `.pdata` xref script/report `r2-tls-manager-context-global-xrefs-20261015.py/.txt` SHA-256 `51A97D10F5A1CADD69D0D97E28161E8DC0AB48FEEDFEFDEC9867118653863819` / `AFCA13415EF2A78EC10649E1B5C20BB49ACBB95E0564E03DB0D749962D11FFBF`。两组 raw files 仅存本机 ignored `local-evidence/n4-r2-readonly-20261015/`。

这次把 §7.61 的“动态 control-block release 未解”进一步收窄为：固定映像有配对验证、受锁计数增减和 TLS context 回收，但业务对象身份、`+0x38` writer/value、计数归零后的其它 owner/removal 路径、这个 acquire wrapper 的实际可达性仍 open。manager final strong owner、successful wait、全项 stop/drain/quiescence、consumer ABI/thread、R3 identity/历史 AV 仍未闭合；本轮没有 QQ/SnowLuma 进程访问、native/callback 调用、目标写入、锁内现场读取、聊天/消息操作或 Cargo tests。R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.64 Guarded global getter caller RTTI and bounded alias census (2026-10-15, fixed-sample static)

继续检查 §7.58–§7.60 的 guarded global child-owner `0x67510A0`。输入仍是固定 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；使用既有校验过的 `.pdata` accessor census，不访问 QQ/SnowLuma 进程，也不执行映像代码。

- 对 `0xB816E8` 的 29 个有效 direct `E8` sites / 25 个 `.pdata` callers 做 strict COL/type-hierarchy 与 vtable executable-prefix 匹配。10 个 caller 对应已识别 primary-vtable entries：`MSFService@nt` slots 10/16、`MSFCoreService@nt` slot 16、`MSFSessionImpl@msf` slots 1/2/4/5/20/22、`TaskManager@internal@msf` slot 3；这些匹配中没有 accessor caller 落在 slot 0。其余 15 个 caller 无 strict-vtable entry match。类型匹配只表示代码地址在相应表中的位置，不自行证明 destructor 语义、调用时机或全体动态调用面。
- 对全部 25 个 caller body 作有界线性寄存器/简单栈槽 alias 跟踪：识别到 48 次 global-field read（`+8` 24 次、`+0x10` 24 次），未发现经跟踪 global-base 指针的直接字段写；22 个 tracked call sites 把从 `+8` 读取的值作为 `RCX` 交给 `0xD32138`。与既有证据一致，`+8/+0x10` 是被读取并提交的 runner pair 路径，而不是此 caller 集合中找到的 teardown 写入路径。
- 两个 hash-gated verifier/report `r2-global-getter-caller-rtti-followup-20261015.py/.txt` 与 `r2-global-getter-caller-full-alias-census-20261015.py/.txt` SHA-256 分别为 `9B92157C20064D61F8750B771CD514AA8752E546EF956B620B6AB87C031D3C22` / `A393A29F4333F3A4CDC95B4E50BC710D4973B26CFBF093FFF2649610C0E45103`，以及 `D5E01C06B643BB9F488C5CBA7BF36DC10B4BBDA2CEFFCCD6D08D5AF54D2CB5B7` / `DBEE692DA1A65FFA5919684C364FB8D74F55996D53EDF7C2499A813ABB3924B0`；raw artifacts 只保存在 ignored `local-evidence/n4-r2-readonly-20261015/`。

该筛查没有识别 global child-owner 的 finalizer、clear path 或 manager last strong release。alias pass 非 CFG/path-sensitive，仅跟踪限定的寄存器/栈槽传递与前四个 Windows x64 参数寄存器，不覆盖 interprocedural effects、indirect/no-`.pdata`、alternate branch、完整 stack argument 或 external owner；vtable no-match 也不是 absence 证明。成功 wait、全 manager stop、callback drain/quiescence、consumer ABI/thread、R3 identity/历史 AV 均开放；R4 不准入，feat-005/006 blocked，产品 NOT READY。没有 QQ/SnowLuma process access、native/callback、目标写入、锁内现场读取、聊天/消息动作或 Cargo tests。

### 7.65 CRT detach pointer-range table cross-reference screen (2026-10-15, fixed-sample static)

为继续界定 §7.59–§7.60 中可能参与 CRT detach 的固定表，复核 RVA `0x4916C00..0x4916D00`。输入仍为同一冻结 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`；此项只读取本机映像字节并扫有效 `.pdata`，不访问进程或运行回调。

- 全 `.pdata` census 扫过 525,295 函数 / 18,889,451 指令，窗口内只有 4 条 RIP-relative 引用：`0x3E6BAA0` 将同一 RVA `0x4916CB0` 同时作为循环 begin/end，`0x3E6BADC` 将同一 RVA `0x4916CC0` 同时作为 begin/end。两个函数体都按 8-byte 项读取非零地址并间接调用，但当前固定映像中的范围长度为零，没有静态项可执行。两 consumer 各有一个 `.pdata` direct caller，位于 `0x3E6AE98` 与 `0x3E6AFA2`。
- 固定映像所有 PE sections 中均没有指向 `0x4916CB0` 的绝对 VA/RVA qword literal。窗口 census 没有找到其它 `.pdata` 指令引用这两处表地址；这只限制有效 `.pdata` 的直接 RIP-relative 表访问，不排除 no-`.pdata` code、运行时/间接写入、其它表或 external owner。
- Hash-gated full-census verifier/report `r2-global-crt-terminator-table-xrefs-20261015.py/.txt` SHA-256 `E1E0CF5D2B6AA1E0E644C3D198C3FDDFC5F30407E51AD1FFAE56E8FB963CDC62` / `9A9427846B6548D7D202C8F8AD901E0E6AAA24D6EA9F13189D9B8ECCB7E530D1`；原始文件只保存在 ignored `local-evidence/n4-r2-readonly-20261015/`。

这两处零长度固定范围没有提供 global child-owner 清理证据，也不等于动态 `_onexit` writer/table 已被排除。manager finalizer/last strong release、成功 wait、全项 stop、callback drain/quiescence、consumer ABI/thread、R3 identity/历史 AV 仍 open；R4 不准入，feat-005/006 blocked，产品 NOT READY。无 QQ/SnowLuma process access、native/callback、目标写入、锁内读取、消息动作或 Cargo tests。

### 7.66 Candidate dynamic detach registry dispatch and initializer callers (2026-10-15, fixed-sample static)

继续 §7.65 的 no-`.pdata` / indirect registration 缺口；输入仍为冻结 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。全量有效 `.pdata` 操作数扫描覆盖 525,295 函数 / 18,889,451 条指令，在 27 个引用中分离出 3 个静态初始化数组引用与 24 个动态 registry 状态引用。

- 动态状态候选为计数/容量 dword RVA `0x6D8E5E0`、槽数组指针 `0x6D8E5E8`，以及相邻状态字节 `0x6D8E5E7`（语义仍未命名）。初始化函数 `0x3E6E574` 分配 8-byte 槽数组并初始化前三个 `0x58` 对象；固定代码路径含 `0x200` 初始容量与至少 3 槽的边界。`0x3EA8078` 在索引 3 及以后惰性申请对象并写入槽位。仅凭代码形状不把它们命名为通用 CRT `_onexit` ABI。
- detach consumer `0x3E6E4DC` 在动态数组指针非空路径转入 `0x3E93AA0`；该 thunk 设置调用标志后转到 `0x3E93C08→0x3E93D04`。dispatcher 按槽遍历非空对象，读取对象 `+0x14` 状态并经 `0x3E93C68` 处理选中项。清理函数 `0x3EA33E4` 处理索引 3 及之后的对象；`0x3E6E694` 组合 dispatch、尾槽清理、前三槽处理并最终清空数组指针。这闭合固定映像中的一个动态 detach registry 候选及其执行/清理代码路径，不识别注册 ABI、消费者线程或其与 guarded manager owner `0x67510A0` 的关联。§7.67 后续确认 `0x3E93C68` 对 `0x3E93AA8` 是直接调用，并将该路径收窄为缓冲 I/O/resource cleanup；“CRT”在本节标题中仅是当时调查假设，不代表已识别通用 CRT `_onexit` 表。
- 静态初始化数组范围候选 `[0x490A2A0,0x490A2E8)` 含 `0x3E6E574` 等非空函数指针，由 `0x3E6AE40→0x3E88698` 遍历调用，遇到非零返回值停止。它与 `0x3E68820` 所用的零长度范围端点地址重叠，不据此认为两个数组用途相同。对疑似注册函数 `0x3E905C4` 的 5 个直接调用者传入 `.rdata` 静态文本地址；这些站点不是已识别的 finalizer callback。
- no-`.pdata` gap screen 在 65,758,720 个可执行原始字节、252,747 个有效 `.pdata` 间隙 / 1,362,192 字节中，没有找到对上述动态状态 RVA 的直接引用。扫描从间隙边界以 skipdata 解码；不能排除嵌入/错位指令、寄存器别名、间接写入、运行期填充、外部模块或其它 registry。
- Hash-gated ignored artifacts（均仅存于 `local-evidence/n4-r2-readonly-20261015/`）：全 `.pdata` 操作数 census script/report `r2-onexit-global-operand-census-20261015.py/.txt` SHA-256 `0561778B41531C8DFDB5F8D438A263C963CC58741DCAE2612C5D4FE752B68C9D` / `04110E9AA2C100775C568F55ADC51A32377142805B17C7B013255303599CF051`；no-`.pdata` gap screen `r2-onexit-nopdata-global-refs-20261015.py/.txt` `30FF1460D246DB6CFB2D0FC79FAE11A839615400F69358F0D507CE9CC4444CB6` / `DA60DFE1734B51C7659AC580B36BE938C5D63B5C26E43EAEA83FFC41D8CF71F6`；注册 caller census `r2-onexit-registration-callers-20261015.py/.txt` `D7DBE16250DFFFD28EB171473E0CA690B0A9870D0232D785F8C25638E7026809` / `50A711DA67D18D555F1CA210D7EB4890ACF656951869C93B4678A7DFCDF7DAF2`；detach/init consumer bodies `r2-crt-detach-consumer-bodies-20261015.py/.txt` `6F23DF455BB87A9BE777E25527DC1E6089D53CCD59503E9CE70D2E05672D3BF0` / `52DAF79E607F1DA608192B3CE62EF9ACE1F9AB773EE522CDD5843837C6C8838B`；initializer data window `r2-crt-initializer-data-window-20261015.py/.txt` `6FED365B90C551855C535EB1B98DB987FB0EC3A324A77AE2B4B72685E7C8F5FA` / `D7EED1CF29D26DF8D3F60CDD7AFDB60D10565330C7D244AA5F8D1C17D151069F`。

本节只识别固定映像 registry 候选及静态调用/清理形状；没有运行 callback，也没有证明实际注册项或 detach 时执行结果。registry callback 关联、与 manager global state 的关系、最后 strong owner、成功 wait、所有 manager entry 停止、完整 callback drain/quiescence、consumer ABI/thread、R3 identity 与历史 AV 仍未知；R4 不准入，feat-005/006 blocked，产品 NOT READY。无 QQ/SnowLuma 进程访问、native/callback 调用、目标写入、锁内现场读取、聊天/消息动作或 Cargo tests。

### 7.67 Candidate dynamic slot path narrowed to buffered I/O resource cleanup (2026-10-15, fixed-sample static)

这是对 §7.66 候选分类的后续校准：冻结 `wrapper.node` SHA-256 仍为 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。此前“经 `0x3E93C68` 间接执行 `0x3E93AA8`”应更正为 direct `E8` call；“CRT detach registry”是搜索假设，不是已确认的 CRT ABI 或 callback registry。

- `0x3E93D04`（`.pdata` `[0x3E93D04,0x3E93DE5)`）遍历 `0x6D8E5E8` 槽数组，按 `0x6D8E5E0` 上界检查各项 `+0x14` 标志，再调用 `0x3E93C68`。`0x3E93C68` 在其 `[0x3E93C68,0x3E93D04)` 函数体内直接调用 `0x3E93AA8`；另一个直接 caller 是 `[0x3E93DE8,0x3E93E28)` 的 `0x3E93E0C`，所以当前有效 `.pdata` census 为 2 sites / 2 functions。没有从这条已解码链观察到由槽提供的任意 callback 间接调用。
- `0x3E93AA8→0x3E93B7C→0x3E9110C→0x3E90DDC` 处理有缓冲数据；`0x3E90DDC` 的导入间接调用解析为 `KERNEL32!GetConsoleMode`（IAT RVA `0x490FDC0`）和 `KERNEL32!WriteFile`（`0x4910470`）。同一路径把 stream/resource 索引用于另一全局分块表 `0x6D8ECA0`。这支持将候选描述为 buffered I/O/resource flush path，而不是 manager callback executor；它不能排除映像其他地方、间接或外部模块还存在别的 callback。
- 候选槽尾项的关闭/释放路径 `0x3EA33E4` 按索引 3 起处理 `0x6D8E5E8` 中的项；符合标志条件时调用 `0x3E93824`，随后释放子对象并清空槽。`0x3E93824→0x3E93964` 继续走对象状态与锁 helper；前三项由 `0x3E6E694` 单独清理，最后释放并清空槽数组指针。另有 `0x3B5C8BD` 将 `0x3E93824` 写入外层对象 `+8`，与它是资源清理方法的解释相符，但不足以恢复完整静态类型。
- 注册侧 `0x3E905C4→0x3E90628→0x3EAAf8c→0x3E8A0AC` 落入构造/资源路径；相关深层 body `0x3E8A1FC` 使用 `KERNEL32!CreateFileW`（IAT RVA `0x490FC40`）、`GetFileType`（`0x490FE90`）和 `GetLastError`（`0x490FEA8`）。5 个 direct callers 位于多个独立代码区域，并传入静态 `.rdata` 元数据；现有证据不将这些站点认作 manager finalizer callback，也不证明它们与 `0x67510A0` 同对象。
- 邻近 dword RVA `0x6D8E5F0` 的有效 `.pdata` 操作数 census 找到 6 个更新点，均为 `inc` 或经 `EDX=1` 的 `add`；其中 `0x3E996D8` 在分配失败分支递增。因此它是用途未明的辅助计数候选，不能解释为 live-entry/refcount、成功资源数或 quiescence 证据。此字段没有做 no-`.pdata` 全映像补扫。
- Hash-gated ignored evidence（仅 `local-evidence/n4-r2-readonly-20261015/`）：dispatch helper bodies `r2-onexit-dispatch-helper-20261015.py/.txt` `E6E7CA71760F6B1F5B9419DD8F5F022062D1EDAC8CE0AF71D5A7C19365CC575B` / `4FD5A72F5E5BABE33FD3C9D314CCE06D9FDE17079996440815BD7556DFD3AC98`；buffered I/O leaves `r2-onexit-resource-io-followup-20261015.py/.txt` `C431242A2069B16EE09F794CF1023577FBF785341076B67476152140CF4D961A` / `76C976604227AF4032527D65B86723D25B0A42F7A0BACD2B3CE245DC4BEE445C`；dispatch IAT resolution `r2-onexit-dispatch-imports-20261015.py/.txt` `7C1CA09D56E84AC5B9A793F0624E884BFAA72884E6854C4C730E9343885B8844` / `B742FDCE562CD18416FBCC2F775769E13A896EDB13269A751B78A7FD7C3381C5`；validated direct callers of `0x3E93AA8` `r2-onexit-buffer-flush-callers-20261015.py/.txt` `E9FFAE8F8B1BB86D586E2C8AFE629F3ED08E5E011F789891C7167FF4BE2D2F21` / `C668C5F9FBEE3EE2A2A5A5D0EE0293C6B60EB1B0AAE5A9CD9D12390D2F101DC5`；detach/cleanup bodies `r2-onexit-detach-flush-followup-20261015.py/.txt` `C339C45B3D62AB9DB08C0605C0F9F1B3CCF49CE92804DBB5330BB4D227AC7A9E` / `38AE935A1F71C2F5F7C6C296758792545B9DC099EEB00BE7BD094E05468073B7`；stream close body `r2-onexit-stream-close-followup-20261015.py/.txt` `CD3281B8E783696AB7E8D812A8A508A931BE28B83F882968B1FFABC278F0F52D` / `964E7826B45EBCBF022C3D96F268084ED7B497660D01565418A55DEC0DE05DF4`；resource import map `r2-onexit-imports-20261015.py/.txt` `3F98EF666E1D5876171987A869F70860F76248E65A08A65BF6DD2E80BBA298B7` / `7B2EBF6E9FBB61F2D71FB4A5F7CC7B5EEE1025E3548EBE217629735E1C133E6B`；registration callers `r2-onexit-registrants-20261015.py/.txt` `2BAC8F96E75E4E26BD6D1A10B043525487FCEE7211B45BC3A1A5B6030BEE613F` / `23D0192B538469EB6ED8A02784B9EF8BE5532EC9118F301BBFA511C204578C73`；counter writers `r2-onexit-counter-writer-bodies-20261015.py/.txt` `CFBE97D75594E5D3CF14B0E776AAD5F8193AB2B66B1FF0AD0B8C1F38E4A367F1` / `638E88D270ED3B632C32B6E714EA2EFA8378B7685AFF461649822BE72135CCF4`。

本节没有识别 `0x67510A0` manager global 的 final owner、析构触发、successful wait、全 manager stop、callback drain/quiescence、consumer ABI/thread；也没有证明该 resource table 对应 QQ manager。R3 identity 与历史 AV 仍 open，R4 不准入，feat-005/006 blocked，产品 NOT READY。仅读本机冻结映像；没有访问 QQ/SnowLuma、调用 native/callback、写目标、读锁内现场集合、执行聊天/消息操作或 Cargo tests。

### 7.68 ThreadTaskManager primary slot 4 bounded alias target (2026-10-15, fixed-sample static)

继续检查 manager 主 vtable 中尚未分类的 slot 4；输入仍绑定冻结 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`。已知 RTTI 对照将 vtable RVA `0x4260448` 识别为 `ThreadTaskManagerImpl`。

- slot 4 的表项为 `0x217DAEA`，无 `.pdata`。从已验证表项做有界入口解码得到 `mov rcx,[rcx+0x20]; movsxd rdx,edx; jmp 0x31F7F8A`。跳转目标也无 `.pdata`；按该精确目标边界的短体解码为 `mov qword ptr [rcx+0x38],rdx; ret`。这仅证明一条 slot-4 field-write 形状，不给该字段或写入值命名。
- 既有 manager 构造体在 `+0x20` 保存输入 pair 首 qword；manager destructor 对 `this+0x20` 调 `0x19044` 清理 helper。当前 follow-up 只解到 `0x19044` 的 `xor edx,edx; jmp 0x1EF48` no-`.pdata` stub；没有据此推出该对象的动态类型或 ownership ABI。该 slot-4 短路径本身没有 stop/wait、queue drain 或 callback 调用。
- 忽略文件中的 hash-gated script/report `r2-manager-vtable-slot4-review-20261015.py/.txt` SHA-256 为 `BDFE4FEF86E8015F981AADA8E7F1A24848529CCED61270E3C256AF158B9BD45C` / `46206604A85BAF7A568B1052F69CB14C6C4AF0559E4A33855FCD762BC1358B28`；路径均在 `local-evidence/n4-r2-readonly-20261015/`。脚本校验输入文件 hash、PE image base、vtable RVA/file offset 和 `.pdata` 边界，只对两个 no-`.pdata` entry 做短解码。

slot 4 分类没有识别 `0x67510A0` final owner、manager final strong release、stop/wait 成功或全局 drain/quiescence，也不改变既有 child `D313D6` stop/join-shaped 路径及 wait-result unknown。R2/R3 仍未闭合，R4 不准入，feat-005/006 blocked，产品 NOT READY。仅离线读取冻结文件，无 QQ/SnowLuma 进程访问、native/callback、目标写入、锁内列表读取、消息动作或 Cargo tests。

### 7.69 Manager stop/wait caller boundary and repeated passive observation (2026-10-15, fixed-sample static + read-only field)

继续在同一冻结 `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0` 下检查 child stop/wait 的直接调用边界。严格 `.rdata` RTTI/COL screen 校验了 15,410 个 vtable starts，并要求 caller 落在对齐的可执行 vtable prefix 中。

- `D313D6` 有 18 个 direct `E8` sites / 12 个 caller functions；只有 caller `0x290C612` 严格命中 `ThreadPoolImpl` primary vtable RVA `0x4300918` slot 3。其余 caller 没有该直接表成员证据；这既不排除 thunk、非虚、间接或外部 owner，也不把 `ThreadPoolImpl` 等同于 `ThreadTaskManagerImpl`，更没有把 caller 连到 guarded global `0x67510A0`。
- `D3355C` 的 2 个直接 callers（`0xC71F9A` 和 `D313D6`）均没有严格虚表成员命中。已解码短体以 `INFINITE` 调用 `WaitForSingleObject`，随后 tail-call `CloseHandle`，不检查 wait 返回值；这仍不是 successful join、所有任务已停止或 callback 已排空的证明。
- Hash-gated verifier/report `r2-manager-stop-wait-caller-vtable-screen-20261015.py/.txt` SHA-256 `A3E5AE4F648D771C092B06C15ADD5863BF4AE7469463DC33E5709878CB6C9AF5` / `07FB610D5F37702E50D8E829F1AAA74C3F21C624C0760D7CD05BDCBA58DFF6B3`；其输入 direct-call report SHA-256 `C1DFBD07A5E294B6DE618254CDF3CF20BA69AF2D0F7FE47DC1C90C124AF1354F`。原始材料仅留在 ignored `local-evidence/n4-r2-readonly-20261015/`。
- 对用户指定的最新 QQ 根进程再做两次外部只读 `observe-msf`：PID `27604` / 创建时间 `2026-10-15T00:59:34.989Z`，两次间隔约 8 分 13 秒；磁盘与预期 `wrapper.node` hash 均为上述冻结值。两次都报告 dispatcher 位于 `wrapper.node`、TaskQueue owner TID 可在该进程线程清单中匹配、TLS index 有效、146 个线程记录、2 次 manager-chain visits、4 个 manager candidates；executor singleton anchor 均未命中。脱敏汇总 SHA-256 `26C327AACFDF08D82CC81BB31BCEC11636D24A97FCBF93CCD9399A71469A1766`，原始报告与 stdout/log hashes 在该本机 JSON 中。该 owner-TID 匹配不是 callback consumer 身份或执行证明；executor anchor 未命中也不支持把该线程归属到响应 callback。

两次 observer 没有调用 QQ/native/callback、没有写目标，也没有读取 `MonitorTask+0x98` 锁内集合或聊天数据。此项没有找到 global owner 的 final release、successful wait 或完整 stop/drain/quiescence；R2/R3 仍 open，R4 不准入，feat-005/006 blocked，产品 NOT READY。

### 7.70 `0x180D30FF0` is inside the frozen wrapper image (2026-10-15, fixed-sample static erratum)

This dated erratum supersedes only the image-scope conclusion in §7.28 (and the corresponding progress, thread-contract §19, and execution-ledger statements). The frozen `wrapper.node` SHA-256 is `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`. PE32+ parsing validates `ImageBase=0x180000000`, `SizeOfImage=0x7419000`; RVA `0xD30FF0` is within executable `.text` (`0x1000..0x3EB7400`) and `.pdata` function `[0xD30FF0,0xD310D5)`. Therefore VA `0x180D30FF0` is an in-image function, not an external-module address.

The hash-gated cross-path decode confirms that `0x4E6DAC` allocates `0x120` bytes and calls `D30FF0`; setup `0xB81758` forwards that returned child through global state and calls `D31686`. Separately, RTTI-bound `MSFThread_xpng` constructor `0x1B63EC0` allocates `0x120` bytes and calls the same `D30FF0` with `r8d=1`; slot 2 `0x1B63FC0` reads the payload at wrapper `+0x8`, calls `D31686`, then `D31A9A`. This establishes an in-image shared constructor/manager-install path and allocation shape, but not that the global payload has the `MSFThread_xpng` RTTI identity or identical constructor arguments.

The RTTI-bound wrapper deleting destructor `0x1B643C0` calls `D31356` for its non-null `+0x8` payload; `D31356` calls `D313D6` and releases fields including payload `+0x110`. No evidence here proves that global-state teardown calls `D31356`, identifies the global payload RTTI/type or control-block ABI, locates the global finalizer or last manager owner, or proves successful wait, full stop/drain, or callback quiescence.

Ignored local verifier/report `r2-global-owner-msfthread-payload-constructor-crosspath-20261015.py/.txt` SHA-256 `C331D0B45FC821047609F81F1677CB84C9AC88EA8CAF442614FDABB5A6DE50D7` / `258A1C2FD0B01C56A7DC114072A6B2320D3AE0E3D360E6BF4F3D15E0EDDE81AB`; both live under `local-evidence/n4-r2-readonly-20261015/`. The verifier asserts the exact wrapper hash, PE32+ image base/size, executable section, `.pdata` ranges and direct call edges. R2/R3 and the historical AV remain open; R4 is not admitted, feat-005/006 remain blocked, and product readiness is NOT READY.

### 7.71 `D30FF0` payload field initializer and validated direct callers (2026-10-15, fixed-sample static)

Follow-up to §7.70 decodes the complete `.pdata` body `[0xD30FF0,0xD310D5)` and all four validated executable-section direct `E8` callers on the same frozen wrapper hash. `D30FF0` receives an allocated `0x120`-byte object in `RCX`, a string-shaped source in `RDX`, and a 32-bit value in `R8D`. The function stores `R8D` at object `+0`, initializes two nested fields at `+0x30` and `+0x68` through the same helper with argument `1`, copies the supplied string-shaped value into `+0x48`, zeros `+0x60`, bytes `+0x80` and `+0x100`, and the pair-shaped `+0x110`; it then calls an initializer on `+0x80`. These are field/action observations, not names or semantic meanings for the fields.

The four direct callers are `0x4E6DAC`, `0xCA94B0`, `0x1B63EC0`, and `0x33D631D`. `0xCA94B0` and RTTI-bound `MSFThread_xpng` wrapper constructor `0x1B63EC0` pass constant `R8D=1`; the other two load the value from their own input pointer. Thus the shared initializer and field layout are established, but equality of caller modes, global payload type/RTTI identity, business meaning of `+0`, constructor ABI beyond these observed inputs, and exact global release path remain unknown. The static `MSFThread_xpng` RTTI belongs to the outer wrapper; it is not evidence that the inner payload has that RTTI.

Hash-gated verifier/report `r2-d30ff0-constructor-fields-callers-20261015.py/.txt` SHA-256 `E47902D742BD9986D362C4A131C22B06F58B7059838975CAADD1C15EF7BED501` / `5E09B5F8DCE2CBF5758EAFE18A3A911DC56D556D8B66C45284E62CAA14D7FACA`, stored only under ignored `local-evidence/n4-r2-readonly-20261015/`. The verifier asserts the image hash, PE base/size, valid `.pdata` body, four exact direct callsites and each Capstone instruction boundary. This narrows the constructor-argument question only; manager final owner, successful wait, full stop/drain/quiescence, R3 identity and historical AV remain open. R4 is not admitted, feat-005/006 remain blocked, product readiness is NOT READY.

### 7.72 `D3355C` second direct caller is a separate handle-bearing object path (2026-10-15, fixed-sample static)

Follow-up on frozen `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0` validates the complete `.pdata` caller `[0xC71F9A,0xC72058)`. Its only direct incoming `.pdata` call is `0xC71F2E` from `0xC71F10`; `0xC71F10` is directly called at `0x5B070` from `0x5B060`. The executable-section scan of instruction-boundary-validated direct `E8` calls owned by valid `.pdata` functions finds exactly two callers of `D3355C`: `0xD31551` in `D313D6` and `0xC7201C` in `C71F9A`.

`C71F9A` saves entry `RCX` as `RDI`, touches `RDI+0x18` through `0x6997E` with `EDX=0`/`R8D=5`, calls `D33222` with `RDI+0x60`, then calls `D3355C` with `RCX=RDI`. The bounded `D3355C` body dereferences its input once, passes that value to `WaitForSingleObject` with `INFINITE`, then passes the same value to `CloseHandle`; it does not test the wait result or clear the input slot. Thus this caller supplies an object base whose first qword is treated as the handle, unlike `D313D6`, which passes the address of child field `+0x10`. `C71F10` invokes `C71F9A` before separate field-cleanup calls, but this alone does not prove its C++ type, owner relation, or that the closed handle was joined successfully.

A strict COL/TypeDescriptor/direct-vtable scan found no `.rdata` vtable entry for `C71F9A` or `C71F10`, and no strict membership for their decoded caller functions `C71F10` / `0x5B060`. The bounded screen does not exclude indirect, thunked, nonvirtual, external, or non-`.rdata` ownership. This separates the helper's second static call path but does not connect it to guarded manager global `0x67510A0`, identify a manager finalizer/last owner, prove wait success, or establish full stop/drain/quiescence.

Hash-gated local verifier/report pairs under ignored `local-evidence/n4-r2-readonly-20261015/`: `r2-secondary-wait-caller-20261015.py/.txt` SHA-256 `CDD888723671E67115B1F07EFECC0E0F428747499A8DDE097712219D2A3FB915` / `65EAABAFBEA2566F1E3E2752DE31748219429D23BE00EAFC94F8EE0E4B64C8EB`; `r2-secondary-wait-rtti-screen-20261015.py/.txt` SHA-256 `620F186DFDFB8F36D2789A352D94722DF13B65665FB5ACBAF35F3BC84C550256` / `959DD9F42F360B311EE8B439DB263286D71462249ECD7BC3D1E5055D415B0285`. The first asserts the frozen image hash, valid `.pdata` starts and exact call boundaries; the second revalidates that image and the first report hash before screening RTTI/COL. Static-only; no QQ/SnowLuma process access, native/callback invocation, target write, locked-list read, message action or Cargo test. R2/R3 and historical AV remain open; R4 is not admitted, feat-005/006 remain blocked, product readiness is NOT READY.

### 7.73 `ThreadPoolImpl` local child setup/stop/delete path (2026-10-15, fixed-sample static)

On frozen `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`, the validated primary vtable at RVA `0x4300918` (COL `0x43009B0`, `.?AVThreadPoolImpl@internal@xpng@@`) has slot 0 `0x290DA46`, slot 2 `0x290C746`, and slot 3 `0x290C612`. Slot 2 reads the object child pointer at `this+0xA0` and at `0x290C85C` calls `D31686` with that child as `RCX` and a local pair-shaped value as `RDX`; it then follows its own success/error branches. This is a static field/call relationship, not proof that the object is the guarded global state's child.

Slot 3 loads the qword at `this+0xA0`, calls `D313D6` at `0x290C667`, then invokes cleanup helper `0x504AEC` on the address of that field. The validated vtable slot-0 deleting destructor `0x290DA46` calls body `0x290C59C`; that body calls slot 3 at `0x290C5B2`, then cleans `+0xA0`, `+0x90`, `+0x48`, `+0x40`, and `+0x28` before tail-calling `0x3FAA`. This establishes a class-local one-child stop/release order for the exact `ThreadPoolImpl` vtable.

The slot-2, slot-3, body, and deleting-destructor functions contain no direct RIP-relative operand to guarded manager global RVA `0x67510A0`. This function-local screen does not cover helper effects, indirect references, runtime state, or other class methods. Neither shared use of `D31686` nor the local `+0xA0` child field connects this `ThreadPoolImpl` instance to the global-state object, proves the global final owner, or closes all manager entries / callback drain / quiescence. R2/R3 and historical AV remain open; R4 stays not admitted.

Hash-gated local verifier/report `r2-threadpool-slot3-stop-body-20261015.py/.txt` SHA-256 `69FA951BE55B7ABCBDA242CF27F19CC42C647AB6416AFC26B546B233D6B0209A` / `D7219ED2B3E38725A78A033431CEA8D94B7D6CC3143A08A61ED646EAD1BC172F`, stored only under ignored `local-evidence/n4-r2-readonly-20261015/`. It asserts the image hash, exact vtable RTTI/slot targets, `.pdata` starts, direct call boundaries, and caller/disassembler output. Static-only; no QQ/SnowLuma process access, native/callback invocation, target write, locked-list read, message action or Cargo test. R4 is not admitted, feat-005/006 remain blocked, product readiness is NOT READY.

### 7.74 `ThreadPoolImpl` construction through `ThreadPoolFactoryImpl` (2026-10-15, fixed-sample static)

On the same frozen `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`, the exact primary vtable `0x40F6348` / COL `0x40F6380` identifies `.?AVThreadPoolFactoryImpl@internal@xpng@@`; slot 1 is `0x173C2BA`. Its unique raw `.rdata` qword reference to that method is aligned at factory slot 1. The exact primary vtable `0x4300918` identifies `ThreadPoolImpl`; its constructor `0x290C4B8` has one valid direct `.pdata` E8 caller, `0x173CA7D` at site `0x173CAA8`.

The validated direct call chain begins at factory slot 1 and continues `0x173C2BA → 0x173C51D → 0x173C96A → 0x173C9F2 → 0x173CA7D → 0x290C4B8`, with instruction sites `0x173C3E1`, `0x173C53C`, `0x173C99C`, `0x173CA21`, and `0x173CAA8`. An exact encoded RIP-relative LEA census found the pool's primary vtable load in its constructor at `0x290C4E1`, and the factory vtable load in `0x173C8F8` at `0x173C900`. The bounded chain functions have no direct RIP-relative operand to guarded manager global RVA `0x67510A0`.

Validated COL/CHD ancestry keeps these types distinct: `ThreadPoolFactoryImpl` derives from `IThreadPoolFactory`; `ThreadPoolImpl` derives from `IThreadPool`, `IThreadPoolController`, and its `enable_shared_from_this` base; `ThreadTaskManagerImpl` derives from `ThreadTaskManager`, `SequencedTaskSource`, and its separate `enable_shared_from_this` base. The hierarchy contains no common project-specific base among the three; inheritance also says nothing about runtime composition or ownership.

This identifies a static factory-to-pool construction route; it does not show that a runtime `ThreadPoolFactoryImpl` instance exists, that the factory or constructed pool belongs to `ThreadTaskManagerImpl` global `0x67510A0`, or that this class owns the manager's final child reference. The factory slot-1 method and pool slot-1 / deleting-destructor entries have no direct E8 callers with valid `.pdata` owners, as expected for virtual entries; the factory vptr initializer `0x173C8F8` also has no such direct caller and no exact REX.W RIP-relative LEA reference in the bounded census. Indirect construction/registration, unowned code, aliases, other address forms, runtime ownership, successful wait, complete stop/drain/quiescence and callback consumer identity remain open. R4 stays not admitted.

Hash-gated consolidated verifier/report `r2-threadpool-factory-to-pool-owner-chain-20261015.py/.txt` SHA-256 `8024440F22D5499AB8B4F98FB66507C5E0F89BD77F558835E052D5D393CBEAC4` / `2F9E7242EF50CE2E7DB88F6316670D0A7BE39C1CB1148893B2DEAF43F4E8D6F6` is ignored under `local-evidence/n4-r2-readonly-20261015/`. It revalidates the fixed image hash, both RTTI/COL vtables and slots, all five direct call edges and instruction boundaries, the constructor's sole valid direct incoming edge, and absence of direct manager-global RIP references in the bounded chain. No process access, native/callback invocation, target write, protected-list read, QQ message action or Cargo test occurred.

The separate COL/CHD hierarchy verifier/report `r2-threadpool-manager-rtti-hierarchy-20261015.py/.txt` SHA-256 `0C879A0E9E46BA735A9330CA363392B5ED0CA312F56BBA5F269DDDA32D526805` / `5D1ADF61EB07BC2BE2E9E2B587FB9BF47ED215EBE5B0CC640CEE835D93429FAD` is local-only under the same ignored directory. It validates the three primary tables' RTTI ancestry and does not infer composition or runtime ownership.

### 7.76 Factory singleton accessor and caller path (2026-10-16, fixed-sample static)

This supplements §7.74. On the same frozen `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`, accessor `0x173C214` references static object RVA `0x6757EB8` and guard RVA `0x6757EE0`. Its local-static initialization path calls constructor-entry candidate `0x173C2A6` from site `0x173C270`; unowned executable gap instruction `0x173C2B5` tail-jumps to initializer body `0x173C8F8`, which installs the validated `ThreadPoolFactoryImpl` vtable at `0x173C900`. All exact object/guard references in the full executable operand census are inside this accessor.

Function `0x9E2898` calls the accessor at `0x9E28C9`, `0x9E291B`, and `0x9E2A5D`, then dispatches through factory slots 4, 1, and 2 (`+0x20`, `+8`, `+0x10`). A corrected whole-executable direct-call scan finds three valid `.pdata` callers of `0x9E2898`: `0x6ED48A`, `0x6FEB66`, and `0x9C142A`. Those callers are reached at `0x6D40EF` / `0x6D41B0`, `0x6EE85B` / `0x6FF31F`, and `0x9C0FCC`, respectively. Strict COL/RTTI binds owner function `0x6D3DCC` to `LoginService@login@nt` primary vtable slot 4; the other containing functions have no strict direct `.rdata` vtable membership in this bounded screen. These direct edges establish a static caller route from a `LoginService` virtual method into the factory-use helper; they do not identify the remaining helpers' runtime entry mechanism.

The per-function caller scripts initially printed `incoming_direct=0` for `0x9E2898` and the three factory-use functions because one compare mixed RVAs and VAs. Those zero counts are withdrawn. The later full executable scans below validate the direct call operands and `.pdata` owners and supersede those fields. This correction covers direct calls/jumps in decoded `.pdata` functions and boundary-decoded executable gaps only.

The new static path and §7.77's passive sample establish that the factory singleton is constructed in the image and its expected vptr is present in the current sampled process. They still do not connect the factory or a returned `ThreadPoolImpl` to guarded manager global `0x67510A0`. The full executable state/guard screen in §7.75 still finds only the known getter references; no final manager owner, successful wait, complete stop/drain/quiescence, callback consumer ABI/thread, R3 identity, or historical `0xC0000005` cause is established. R4 remains not admitted, feat-005/006 remain blocked, and product readiness remains **NOT READY**.

Hash-gated local verifier/report pairs under ignored `local-evidence/n4-r2-readonly-20261015/`: full factory/accessor reference screen `r2-threadpool-factory-executable-owner-ref-screen-20261016.py/.txt` SHA-256 `B4E3D3C4D0F6C18548EF1F7BE66DF3C8AD8F22CE4F050483D3F81D3E870D258B` / `5AE00E583A927BF9FA0AA4528978A1871A75269B4278BF205DFBC0E97BB6D4F1`; accessor body `r2-threadpool-factory-constructor-caller-20261016.py/.txt` `0648713A3E0E7FF025EF498D41F94B41C0DE127455128BDAE3BC35C7CC955CE9` / `4BE0D81B36B87DE77EE36BB36D9613479FE4A0F003565B31958F51640EE3A410`; accessor/call-window screen `r2-threadpool-factory-static-accessor-callers-20261016.py/.txt` `61F3672DC6997837084F45C6B8D467A49DB1A0F7169084E1F3546ACE1D5A943A` / `62F67E45363AF4555CBD197C2A47C1E3C2D5D860FB439EC4AA069A9DA9C4161C`; corrected executable xref to `0x9E2898` `r2-threadpool-factory-client-all-exec-xref-20261016.py/.txt` `324BD85C4B04E468FF6B0211D765E46B03A06D2F10D72B2754949FD6F406CE76` / `E47C78EE66FD26770BCB921B0EE4EF3CD2FCFC7E2A8B1AA45C42479570BD7C24`; corrected xrefs to the three factory-use functions `r2-threadpool-factory-client-parent-xref-20261016.py/.txt` `BFDFA315D9248DF81F65ACED24588E08029C1BA71F4DDACEF5200489F2DA7A51` / `89A9F02E4E2EF57587975BA7ADAE34DA258D4B33011716D8969D14601FFAA037`; and strict RTTI membership `r2-threadpool-factory-parent-rtti-20261016.py/.txt` `CE7825D4D522D9703340CA4C07187B6D6C43C10CEB44D3B98C0C7FB2BBA6C033` / `9316A99A1E31576E2E3BDA16C5014506CF8DB13673B9C080F7FC4D50EBAD4885`.

### 7.77 Current-root factory, TaskRunner, and watcher sample (2026-10-16, passive read-only)

The latest independent QQ root was selected again from the current process tree. Its process creation identity and wrapper path matched across the factory, TaskQueue owner-TID, and outer-watcher reports; the on-disk `wrapper.node` SHA-256 remained `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`. Two reads 250 ms apart found the static factory object's first qword equal to loaded-module base + factory vtable RVA `0x40F6348`, with stable raw init-guard bytes. In the same process, the raw manager-global word at `0x67510A8` equalled the TaskRunner pointer independently read from the dispatcher anchor. The manager-global word at `0x67510A0` remains untyped; no pointee-type claim is made from it.

The bounded TaskQueue observer found a stable `TaskQueue+0x58` owner TID present in the 75-thread process inventory. A separate two-pass outer watcher sample resolved `TaskRunner`, `ThreadTaskManagerImpl`, and one `MonitorTask` outer node; it also sampled TaskQueue strong count `1`. The same-root join verifier checks process identity, the factory-vptr match, global-word-to-TaskRunner equality, watcher RTTI/count, and owner-TID membership. These are passive observations, not a synchronized lifetime pin: they do not prove callback execution/consumer identity, manager final ownership, wait success, full drain, or quiescence. No `MonitorTask+0x98` inner collection was read; no target writes, native/callback calls, or message actions occurred.

A preliminary probe treated the raw manager-global word at `0x67510A0` as a manager-object pointer and read one qword at that address; it did not match the expected wrapper manager vtable. Because the field type was not established, that interpretation was discarded. The accepted joined report treats both manager-global words as raw values and does not dereference either. Observer and joined-summary hashes: factory observer/report `factory-manager-global-passive-observer-20261016.ps1` / `factory-manager-globals-20261016-005017.json` `164302DF42E632E693BE9D4A1ED2CB9E2BEE2936278892732173474CF25F60BF` / `CFBCE208DDC325F60CD4023B320526E47C683507E7EDD5B9B56DDC35A0D63EF9`; outer watcher observer/report `watcher-chain-observer.ps1` / `watcher-chain-20261016-005033.json` `C6139B3E126D144C656BAF8B8C6D46D2BF32D30AFFACFAEDF31DF9CB43E51193` / `88D52B65FC45D1ED41D5C6A6825DE142A8C4EF2DF7071C05194C603A2C42AF80`; owner-TID observer/report `taskqueue-owner-tid-observer-20261015.ps1` / `taskqueue-owner-tid-20261016-005045.json` `735FF96F51975C335AB88014283C267292208B2A8CC390B45DC8F0BA40DB5D1E` / `E2FBB1A0917398254282EA93DA67DF912FA0A9638F386C20A2096FA38FF03704`; joined verifier/summary `r2-current-root-factory-manager-passive-join-20261016.py/.json` `C88370DD1F2DE993171EBBF9AF8C34BDBC5E86E894F6D317A50EBC72A424BDBD` / `D267DCA3984268A1D8AB33849FAFF74AEBAA40D43B485DEA9751EFCAB3669488`.

### 7.75 Full executable reference screen for guarded manager state (2026-10-16, fixed-sample static)

Follow-up on §7.58's guarded global child-owner candidate `0x67510A0`, its adjacent pair fields `+8/+0x10`, and guard `0x67510B8`. The input is still the exact frozen `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`; this pass reads only the disk image.

- A hash-gated Capstone pass decoded all 525,295 valid `.pdata` functions / 18,889,451 instructions and searched both RIP-relative memory targets and exact address immediates for the state/guard RVAs and preferred VAs. It found exactly six references, all six already localized inside getter/accessor `0xB816E8`; no exact immediate reference was added.
- The same pass covered 65,758,720 executable raw bytes, including 252,747 `.pdata`-unowned gaps / 1,362,192 bytes. Boundary-sequential skipdata decoding found zero references to the four state/guard addresses in those gaps. This extends the prior RIP-relative-only gap scan with exact-immediate checks.
- Hash-gated verifier/report `r2-manager-state-all-exec-ref-screen-20261015.py/.txt` SHA-256 `D7008547320D8BDA04658990A317D36D6977C12DD4C248EC02E92E11F2C1580E` / `85ECA380191173EA62C7BCD142286AA4EEF744B13BB583E126A0BF539E18E2F5`, stored only in ignored `local-evidence/n4-r2-readonly-20261015/`.

This closes only the decoded direct-reference recall for the fixed image. It does not find a global-state finalizer or exclude indirect/computed aliases, embedded or misaligned code, runtime registration, external owners, or process-exit behavior. The identity of the last strong owner, successful wait, full stop/drain/quiescence, callback consumer ABI/thread, R3 identity, and historical `0xC0000005` cause remain open. No binary code was run and no QQ/SnowLuma process was accessed by this static pass; R4 remains not admitted, feat-005/006 remain blocked, and product readiness remains **NOT READY**.


### 7.78 Guarded manager-state accessor direct-caller map (2026-10-16, fixed-sample static)

On the frozen `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`, an executable-byte scan found 29 raw E8 candidates to accessor `0xB816E8`; all 29 decode as instruction-boundary direct calls inside valid `.pdata` functions, with no unowned candidate. The 15,410-header strict COL-bounded primary-vtable map classifies the typed subset as MSFService slot 10 (one call), MSFService slot 16 (two calls), MSFCoreService slot 16 (two calls), MSFSessionImpl slots 1/2/4/5/20/22 (one each), and TaskManager slot 3 (two calls). The remaining 16 caller functions have no direct strict-vtable-entry membership in this screen.

The two state-setup calls to `0xB81758` remain exactly the calls from MSFService slot 16 and MSFCoreService slot 16. No validated direct caller of the accessor was mapped to slot 0 or 1 of a strict primary table in this census. This narrows the direct static read/setup surface; it does not establish that no destructor, shutdown hook, indirect alias, non-primary base path, process-detach action, or external owner releases global state `0x67510A0`. It also does not prove a runtime caller ran, manager final ownership, successful wait, stop/drain, callback execution/consumer thread, or quiescence.

Hash-gated verifier/report under ignored `local-evidence/n4-r2-readonly-20261015/`: `r2-manager-state-accessor-all-callers-20261016.py/.txt` SHA-256 `AC51F1D688AF336F2CDA7A273F1594C29529D0F9EF056B19C95FDFFD5F317FC8` / `7EED22CB64514813781A4A0FC1594799A9B7E10E3C1B5B246E354747736D1183`. The report includes the exact validated call sites and the full slot inventories for the two service tables. No binary code was executed and this static pass accessed no QQ/SnowLuma process, invoked no native/callback, wrote no target memory, read no locked inner list, and performed no message action. R2/R3 remain open; R4 is not admitted, feat-005/006 remain blocked, and product readiness remains **NOT READY**.

### 7.79 Caligo delegate manager assignment correction (2026-10-16, offline)

The installed QQNT 9.9.33-52230 `wrapper.node` still hashes to `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`. The retained `0x54CE00` disassembly (SHA-256 `485025344474889FA876C82081B36C41610CE33307DBAE5865FB45E874E7CF00`) passes the target manager slot as RCX to both calls: first `op=3` with the target wrapper as R8, then `op=1` through the source manager pointer with the source wrapper as R8 and the target wrapper as the fifth argument. Both calls pass size `0x10`. The source pair is an indirect call target, not the RCX output slot.

The Rust `consumer_manager` had incorrectly grouped `op=1` with `op=3`, so an assignment produced an empty target manager/invoker pair instead of copying the cookie and a callable pair. The offline repair copies the two cookie qwords and installs the Rust manager/invoker at the target wrapper for `op=1`, leaves the source wrapper intact, treats `op=3` as POD cleanup without rewriting the pair, and uses the profiled QQ empty pair on `op=2`. The existing callback test now replays the `0x54CE00` call shape and verifies the copied delegate remains callable. A callback capture also records `GetCurrentThreadId` in the preallocated slot; the resident worker reports both `callback_tid` and `drain_tid`. This enables later thread attribution but supplies no actual QQ callback thread sample.

These are source and offline-test contracts. They do not identify the QQ dispatcher owner or final release, prove a successful wait or global stop/drain/quiescence, establish a response/CPB strong identity, or explain the historical research `0xC0000005`. R4 remains outside admission, feat-005/006 remain blocked, product **NOT READY**.

### 7.80 Bounded response UID diagnostic capture (2026-10-16, offline)

The fixed-hash `0x73BD6C` ABI already recorded in §7.3 passes the response `msgUid` string as its fifth argument. `consumer_invoke` now copies that borrowed libc++ string into a preallocated callback slot with the existing bounded self-process reader; the resident worker writes capture state, byte length, and domain-separated per-process keyed fingerprints for both the response UID and local request cookie. This keeps the two callback values together in diagnostics without writing either raw value. The callback performs no allocation, formatting, lock wait, or file I/O. Invalid short lengths above the 22-byte SSO capacity, unreadable strings, and strings above the 256-byte capture budget fail closed. Volatile wipes clear the callback's stack copy after enqueue (also on overflow), then the worker's copy and slot before release.

This fingerprint is diagnostic-only: it is neither the locally generated callback cookie nor a business/native message ID, and it is not passed to the response ledger. The synthetic tests cover short and heap strings, empty/over-budget/unreadable spans, malformed SSO, and slot lifecycle. They do not prove the QQ callback fires, the field layout on a live process, or equality with a CPB field. R3 strong identity and UID→UIN remain open; no field action or R4 admission follows from this instrumentation.

### 7.81 One-shot G3 callback cookie correction (2026-10-16, offline after field sample)

The user-authorized UIN-only G3 request on PID 40240 did not appear in the selected chat. Its 90-second resident drain recorded no `consumer_result`, `consumer_worker_summary`, or `consumer_orphan_callback`; the target remained alive/responding and no crash was observed. This is delivery unknown. Source review then found that the research-only direct G3 sender built its delegate with `this = ConsumerCtx*`. `consumer_invoke` resolves that pointer as a context but intentionally rejects it as a per-send cookie (`has_cookie` is false), so this delegate could not produce a response result row even if QQ invoked it. This explains the instrumentation gap for this delegate shape, not the absent message or whether QQ invoked the callback.

The direct G3 path now allocates one immutable context generation and one process-local atomic nonce per request; the cookie carries `{generation, nonce}`. This selects that request's report path while keeping the callback cookie distinct from the context address. Contexts remain pinned for possible late callbacks; after the fixed 16-slot registry fills, new probes fail closed instead of reusing a live generation. Regression coverage invokes the generated delegate, drains one captured response and verifies the worker diagnostic. The focused consumer suite passed 11/11; the second `init.ps1 -Mode all` run passed docs, default and research suites. The first full-suite attempt failed once at link with LNK1104 for `target-harness/debug/deps/caligo_bridge.dll`; the immediate retry passed, and the transient failure cause was not established.

At this checkpoint, the QQ process still had the prior pinned bridge build loaded at `target-harness/debug/caligo_bridge.dll`; the corrected build had not yet been field-validated and no retry was made. The later single corrected-cookie sample and normal-restart release are recorded in §7.82. This fix is research-path callback observability only. It does not close the production callback ABI/OS thread, manager final owner, successful wait, QQ-wide stop/drain/quiescence, R3 stable response/CPB identity or UID→UIN, or the historical research `0xC0000005`. R4 remains not admitted, feat-005/006 remain blocked, product **NOT READY**.

### 7.82 Corrected-cookie G3 field sample and restart (2026-10-16)

Under the user's explicit one-send authorization, the corrected research DLL SHA-256 `F5547E5E999A2BAF63A3B77D1325E7DD333BF8F63D0AA7AAEBA0A3319A64F7FE` was loaded into QQ root PID 34760 on the frozen QQNT 9.9.33-52230 image. The manifest and designated-instance gates passed; one UIN-only G3 request reached `g3s_done` with all recorded send stages `ok:true`. The callback report had no `consumer_result`, `consumer_worker_summary`, or `consumer_orphan_callback` after 90 seconds. PID 34760 remained alive/responding during that window and no process crash was observed. This exercises the corrected probe in a real process but supplies no callback invocation/thread/identity observation and does not establish server acceptance or delivery.

The user later reported no reply and a normal QQ restart. They did not separately state whether the outgoing text was visible, so visibility and delivery remain unknown; the request was not retried. Read-only inventory confirmed PID 34760 exited and current wrapper-loaded roots PID 34532 and PID 47560 are alive/responding with the frozen wrapper hash and no Caligo module. The pinned image was released by normal process exit. Sanitized field and restart records are ignored under `local-evidence/n4-r4-g3send-34760-20261016/`. The sample does not validate the private OneBot workflow, prove callback non-invocation, close R2 manager/owner/wait/quiescence or callback-thread gates, close R3 strong identity/UID→UIN, explain the historical research `0xC0000005`, or admit R4. Product remains **NOT READY**.

### 7.83 HandleScope ABI/LAB follow-up and ProfileService argument evidence (2026-10-16)

The exact frozen QQNT.dll hash remains `F6F35B8517DA5C38ACE8D4B6ABAD1ABED42F61C8E706D23192A22D91C7620E59`. Its exported `HandleScope` constructor at RVA `0x1F2B10` uses `RCX=this`, `RDX=isolate`, stores the isolate at `this+0`, the previous next pointer at `this+8`, and the previous limit at `this+0x10`, then increments isolate state at `+0x240`. The destructor at RVA `0x1F2AB0` reads the same fields, restores the isolate state and decrements the counter. This verifies the 0x18-byte touched object layout and matches the `FnScopeCtor`/`FnScopeDtor` call signatures in `qq_v8::exec_script`; its 0x40-byte stack allocation is sufficient. A hash-asserting Capstone listing is kept locally at `local-evidence/rea-uid-mapping-20261016/qqnt-handlescope-constructor-destructor.txt`.

`exec_script` now rejects a null isolate before calling any V8 export. Three injected-symbol LAB tests pass: sret slots and callback ABI flow through the shared implementation, context/scope cleanup order is balanced, missing context avoids scope construction, and null isolate makes no V8 call. Command: `cargo test -p caligo-bridge --features research exec_script_lab_tests --locked --offline --target-dir target-harness`. This synthetic harness verifies the caller-side contract; it does not reproduce the Crashpad state where saved `RAX=0` and ZF is clear while the stack-local scope contains a nonzero isolate. The exact incident bridge DLL/PDB remains unavailable, so the historical AV cause is not closed and the field stop boundary remains.


### 7.84 Correction: NTEngine child-constructor call target (2026-10-24, fixed-sample static)

This corrects the target address in §§7.14 and 7.27. The fixed-hash `wrapper.node` call at RVA `0x9C1309` is raw bytes `E8 D4 FE 36 00`, which resolves to VA `0x180D311E2` / wrapper RVA `0xD311E2`. The earlier address `0x180D3111E2` included one extra hexadecimal `1`; its external-image attribution and the resulting “cannot identify this child constructor” limitation are withdrawn. Ghidra's 48-byte prefix at `0xD311E2` matches the pinned image, and the body initializes the `0x120`-byte `NTSdk` child.

The NTEngine constructor stores that child at `this+0xE0` and directly calls `0xD31686` at RVA `0x9C0EE1`. That helper installs the `ThreadTaskManagerImpl` holder at the child-relative `+0x110`; `0xD31AC0` then dispatches through manager slot 1, matching the earlier exact-hash TaskRunner crosswalk in [Phase20](2026-10-17-qqnt-msgservice-taskrunner-chain-phase20.md). These are static edges only. They do not prove runtime construction, request execution, task-consumer identity, a cache write, or an alias to `openIDToUin +0x58`; owner lifetime, stop/drain/quiescence and product readiness limits remain.

Reproducible evidence: [Phase1762](2026-10-24-qqnt-openid-singleton-construction-phase1762.md), with the hash-gated callsite parser and analysis-disabled Ghidra output under ignored `local-evidence/qqnt-core-map-20261016/`. No QQ process or native call was used.

### 7.85 MonitorTask registration, worker state handoff and slot-3 crosswalk (2026-11-06, fixed-sample static)

This later static crosswalk refines the open items recorded in §§7.43–7.44 and §§7.51–7.52. It uses the same frozen `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`; no runtime process or lock-protected live list was inspected.

- Thread entry `0xD31B7E` loads the same child `+0x110` manager, dispatches primary slot 2 to set TLS, then under child `+0x100 != 0` calls `0x217D710`. First lazy MonitorTask creation stores it at manager `+0xE8` and calls `0x31FA36E` with manager `+0x48`. After a TLS check, `0x31FA36E` dispatches the TLS receiver's primary slot 5 through `0x176CE8E`; RTTI binds that receiver to `ThreadTaskManagerImpl` and slot 5 to `0x217DAF6`, which inserts the listener into manager `+0xD0`. This is a conditional static path; actual guard/TLS/list execution remains unobserved. Phase1830's no-direct-E8 caller census remains correct and is compatible with this indirect virtual path.
- Worker slot 1's secondary `this` has RTTI base offset `+8`. Its `this+0x58` state is complete manager `+0x60`; for a valid state, helper `0x4228C6` returns that pointer to observer fanout after the worker compares/synchronizes it against a local stack record. The zero-state fallback at `0x99C5` remains unparsed. Thus MonitorTask's fanout input is the manager state area on the nonzero branch, not the local stack record address. The state can include a copied callable pair, but this does not identify a particular `GetRoamMessageList` holder instance.
- Setup `0x31FA3C6` conditionally creates key 1/2/4 payloads for `MonitorTaskProcessorFile`, `MonitorTaskProcessorLog`, and `MonitorTaskProcessorReport`. The tree node writer stores payload at `node+0x28`, the field read by MonitorTask slot 1. Their slot-3 targets are respectively the `ret; int3` stub `0x28F0`, Log method `0x3B357F4`, and the same no-op stub. Log slot 3 replaces the incoming `RDX` with selector 5, reads a fixed config bit, then stores the low bit to receiver `+0x18`; it does not invoke the record callable. Runtime flag values and actual tree members remain unknown.
- These built-in MonitorTask processors are not application callback evidence. Other manager observers may exist; no callback execution/result/thread, exact holder arrival, final release, complete drain or shutdown quiescence is established.

The hash-gated script and A/B output are under ignored `local-evidence/qqnt-core-map-20261016/`: auditor SHA-256 `0D5A9A3249A0A1720EEFBEAB0F9F5B036B6FF49D8013EFE976DDD88A8F13CE83`; byte-identical A/B SHA-256 `CB4240D08FA149BB1877EA82EDB72477B9FFC7E5F650B3C62F97FCD053EF625C`; 132 assertions per run. The focused report is [Phase4137](2026-11-06-qqnt-msgservice-monitor-task-typed-dispatch-and-registration-phase4137.md). Static/offline only; no QQ process, IPC, native call, ASAR/JavaScript execution, Cargo build/test, or injection occurred.

### 7.86 Additional slot-5 listener candidates and ThreadHangTarget behavior (2026-11-06, fixed-sample static)

On the same frozen `wrapper.node` SHA-256 `63112AB9161E127F5F7E17998A7196E143808923FB54CBBF7B4E21426187A5F0`, Phase4138 found two additional direct callsites of the generic slot-5 thunk. Phase4139 binds `0x9E0FAE`'s object to RTTI `.?AVThreadHangTarget@thread_hang@nt@@` at primary vtable `0x3FB0F28`; slot 1 is `0x9E0DC2`. On the conditional registration path, `0x9E0FAE` obtains a TLS-derived receiver through `0x176CE60`, stores it through a helper at `this+0x30`, then dispatches slot 5 with `this` as the listener. The TLS receiver is not joined to the MsgService worker's exact manager instance.

The fixed slot-1 body passes its input pointer to `0x9E0EF6`, which copies the first `0x20` bytes to `this+0x70/+0x80`. The slot-1 and input-copy bodies have no indirect call/jump instructions, and the input-copy body does not read input `+0x20`. The follow-up helper `0x6997E` receives selector 5 and exchanges byte `[this+0x60]` with 1. This static body does not directly invoke a holder callable; runtime input identity and execution remain unknown.

Phase4139 also corrects Phase4138's `0x9E1787` source-offset wording: its bounded body passes incoming `RCX` unchanged to weak-pair helper `0x3F70`, while a valid-`.pdata` direct E8 scan finds no direct caller for `0x9E1787`. The containing object and weak-pair field offset are unresolved. The separate `0x33DC019` caller remains untyped and not bound to the MsgService manager. Neither candidate proves runtime registration or dispatch. Holder arrival, result/Promise/message correlation, callback thread, final release and shutdown drain remain open.

Phase4139 auditor SHA-256 `720F2D35B040E49D1AEFADCC5EAA8791161EF9374DA0BA75D5ECFB55FC3DE744`; its byte-identical A/B result SHA-256 is `37FEAC4F58CF5CD8D9C091AB85795B0BEB24DB545EEE0C56B257C9EAA3375D5E`, 49 assertions per run. See [Phase4139](2026-11-06-qqnt-msgservice-thread-hang-target-observer-phase4139.md). Static/offline only; no QQ process, IPC, native call, ASAR/JavaScript execution, Cargo build/test, or injection occurred.

### 7.87 Weak-pair storage tail adapters (2026-11-06, fixed-sample static)

Phase4140 confirms a direct E9 tail adapter into each additional slot-5 candidate: `0x9E1772` jumps to `0x9E1787`, and `0x33DC004` jumps to `0x33DC019`. Both adapters call `0x40634`; that helper prepares storage/capacity arguments for a fixed helper that checks a 16-byte range and 8-byte alignment before returning the adjusted pointer in `RAX`. The helper has no `.pdata` owner or recovered symbol, so this records its bounded byte behavior without assigning it a library or source name.

The adapters have no validated direct `.pdata` E8/E9 callers. The target functions therefore have no validated E8 caller but do have one E9 caller each. No indirect invoker, source object/field for adapter storage, execution thread, or runtime reachability is identified. `0x9E1787` reaches the Phase4139 `ThreadHangTarget` path, whose TLS-derived manager is still not joined to MsgService's worker manager; `0x33DC019` remains untyped. Neither path proves listener registration or callback delivery.

Auditor SHA-256 `05631386B8D655643C92B1D928DDD5D4D0D4FEB3733B175B4FB22A5997E1C291`; byte-identical A/B SHA-256 `B6BB6C2A9577C0AA4B7F1B00C709428C4BFBCDD5D83FFA0A9769465383AB8BE2`, 43 assertions per run. See [Phase4140](2026-11-06-qqnt-msgservice-weak-pair-storage-tail-adapters-phase4140.md). Static/offline only; no QQ process, IPC, native call, ASAR/JavaScript execution, Cargo build/test, or injection occurred.

### 7.88 Tail-adapter fixed-address reference census (2026-11-06, fixed-sample static)

Phase4141 scans all 525,295 valid `.pdata` owner streams in the frozen `wrapper.node` and decodes 18,889,451 instructions with no gaps. It finds no immediate reference, no RIP-relative pointer cell containing the preferred VA or RVA, and no exact VA-qword/RVA-dword occurrence in non-executable raw sections outside `.pdata` for adapter entries `0x9E1772` or `0x33DC004`. The two raw RVA hits are the adapters' own `.pdata` BeginAddress metadata records, not caller or function-pointer evidence.

This only bounds address materialization in the fixed wrapper bytes. Dynamic address construction, another PE, generated code, and runtime-created storage remain outside the scan; the result does not prove either adapter unreachable. Their invokers and storage origins remain unknown, as do runtime registration, callback/thread/lifetime, UID/UIN, transport, and delivery.

Auditor SHA-256 `96ED14692D408FD0B8DCC1B0898B94BFE80A66E15AE33366DCE8E48EDAD2098F`; byte-identical A/B SHA-256 `33CC53181E22916065A977D05C71E4E2E7E5F6B4EACCE6DA1C437D10AA57F802`, 10 assertions per run. See [Phase4141](2026-11-06-qqnt-msgservice-tail-adapter-address-xref-census-phase4141.md). Static/offline only; no QQ process, IPC, native call, ASAR/JavaScript execution, Cargo build/test, or injection occurred.

### 7.89 Tail-adapter relative-data reference census (2026-11-06, fixed-sample static)

Phase4142 scans 43,044,352 bytes across nine non-executable raw sections at every byte offset for signed 32-bit encodings relative to the cell RVA, cell-end RVA, or section RVA that resolve to adapter entries `0x9E1772` and `0x33DC004`. It finds no candidates. Together with Phase4141, this bounds several common fixed-image encodings but does not test arbitrary table bases, transformed addresses, executable data, other PEs, generated code, or runtime-created storage. It does not prove either adapter unreachable.

Auditor SHA-256 `0C925E65C6C726BACDC9EC4A91D31A066AF0BB94D3C90E86A31D5415CBA05E1A`; byte-identical A/B SHA-256 `87C86232A8C850732DA12B515799947084F7C20AC8DA95719C274FD22FC20EAE`, 7 assertions per run. See [Phase4142](2026-11-06-qqnt-msgservice-tail-adapter-relative-data-census-phase4142.md). Static/offline only; no QQ process, IPC, native call, ASAR/JavaScript execution, Cargo build/test, or injection occurred.

### 7.90 MsgService `XV` descriptor selection boundary (2026-11-06, fixed-cache static)

Phase4143 joins module `99990`'s fallback `o.XV[t][n]` read to module key `27218`'s `XV` export slot and to boilerplate object `107085`. The same boilerplate contains the `nodeIKernelMsgService` own key and Phase4124's 298-entry method table with matching descriptor candidates for three message methods. But the recovered first-level key always contains `/`, while none of the 159 boilerplate own keys does. Under the unmodified own-key set, the fallback cannot select that MsgService table; the descriptor matches therefore do not prove adapter selection.

This is a static schema/path mismatch, not a runtime-failure claim. The `XV` runtime instance, mutation/proxy/inherited behavior, alternate route/branch, actual provider registration, selected `g`, native `p[n]` receiver, and its identity with `getMsgService` remain unproven. Callback/thread/lifetime, UID/UIN, message correlation, transport, and delivery remain open.

Auditor SHA-256 `AD9CF354AE20D658091A506446B8B4248893F38C7DA9A9A5430790537125C43E`; byte-identical A/B SHA-256 `BD39040B4FABD8D152FD50A092DCFFDD27B6414A916A14A20077B72CC7318022`, 30 assertions per run. See [Phase4143](2026-11-06-qqnt-msgservice-xv-descriptor-selection-crosswalk-phase4143.md). Static/offline only; no QQ process, IPC, native call, ASAR/JavaScript execution, Cargo build/test, or injection occurred.

### 7.91 Slot-5 candidate post-dispatch state byte (2026-11-06, fixed-sample static)

Phase4146 follows the successful weak-lock path in `0x33DC019`: after the TLS-derived receiver's slot-5 call, the body invokes the selector-5 helper on the locked object at `+0x18` with zero, and the helper exchanges the byte with `DL`. The net bounded effect is to clear that byte. This does not identify the object's type, the byte's role, the TLS receiver's manager, or runtime reachability; no ready-output callable is read or invoked in the bounded owner. Treating the byte as a callback or holder state would exceed the evidence.

Auditor SHA-256 `6E3C2DD41FCA6A10FBDCF8359B7FFA06A12EBDFC39F82DCA91948B4BB7973268`; byte-identical A/B SHA-256 `5D7DD3765A5D5B6C49F308D70BC75168FAA3EE2BC8599E753D0383EA8F8E93D4`, 41 assertions per run. See [Phase4146](2026-11-06-qqnt-msgservice-slot5-candidate-state-reset-phase4146.md). Static/offline only; no QQ process, IPC, native call, ASAR/JavaScript execution, Cargo build/test, or injection occurred.
