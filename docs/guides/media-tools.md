# 媒体常规工具边界

## 范围

`caligo-core::media_tool` 运行显式配置的本机媒体工具。固定参考合同为 `7cf025f` 的 `FFmpegExecAdapter`/`IFFmpegAdapter` 操作边界。

支持的操作是：

- `probe`：从 `ffprobe` JSON 读取 `format.duration` 和首个视频流的 `width`/`height`。
- `audio_to_pcm`：输出 24 kHz、单声道、signed 16-bit little-endian PCM。
- `pcm_to_container`：将上述 PCM 输出为固定枚举 `wav` 或 `ogg`。
- `thumbnail`：在固定 1 秒位置输出固定枚举 `png`，不造默认 `100x100` 或 `60 seconds` 降级结果。
- `silk-wav` / `silk-ogg`：通过可选的 `silk_decoder` 将 SILK v3（含腾讯前缀）解码为 24 kHz 单声道 s16le PCM，再交给现有容器转换。

## 配置合同

生产构造必须同时配置 `ffmpeg` 与 `ffprobe`：

- `executable`：绝对路径，必须是本地 regular file，不接受符号链接。
- `sha256`：64 位十六进制文件哈希；执行前逐字节校验，坏 hash fail closed。
- `version`：固定工具版本字符串，生产构造执行已核对 hash 的程序 `-version` 并验证输出包含该值。
- `license`：用户声明的 `name`、`source`，以及可选绝对 `path`。许可证路径/来源不执行认证、不联网。
- `work_root`：绝对路径。每个任务创建独立目录，任务结束自动 cleanup。
- `timeout_ms`、`max_stdout_stderr_bytes`、`max_input_bytes`、`max_output_bytes`：均必须为正数。

可选的 `silk_decoder` 与 FFmpeg 工具使用相同的路径、SHA-256、版本和许可证声明。解码器接口为 `--version` 和 `decode -i <file> --sample-rate 24000 -o <pcm>`。离线验证使用 [rust-silk 固定提交](https://github.com/wangnov/rust-silk/tree/7d4fd9eb265ac44b6d76bda4002d327651a022c0)。Caligo 通过独立子进程调用它。

工具参数由 Rust `Command` 直接传递，不经过 shell。输入必须是绝对路径指向本地 regular file，不能是目录或符号链接；只检查 basename 存在，不把不受信 basename 拼进命令行选项。输出文件会再次进行 regular-file、大小和固定 magic 校验。

## 进程与资源合同

stdout/stderr 由独立读取线程并发消费，共用总字节上限，避免单管道堵塞。超限、超时或 `StopToken::stop()` 后，子进程执行 kill 并 wait；reader 线程随后 join，任务目录最终清理。非零退出不会被包装成成功结果。文件输入、输出和 PCM 都有明确 byte limit。

`probe` 的 JSON 不完整、duration 非有限/负数或尺寸为零时拒绝。PCM 必须非空且为偶数字节数；WAV 必须以 `RIFF....WAVE` 开头，OGG 必须以 `OggS` 开头，PNG 必须以标准八字节签名开头。

## 正常入口

正常 CLI 入口为：

```text
caligo-media --config <absolute-json> probe --input <absolute-file>
caligo-media --config <absolute-json> pcm --input <absolute-file> --output-root <absolute-dir>
caligo-media --config <absolute-json> thumbnail --input <absolute-file> --output-root <absolute-dir>
caligo-media --config <absolute-json> silk-wav --input <absolute-silk> --output-root <absolute-dir>
caligo-media --config <absolute-json> silk-ogg --input <absolute-silk> --output-root <absolute-dir>
```

配置文件本身不执行下载或安装。CLI 转码使用显式配置的解码器，QQ 下载和上传的验收另见执行记录。

单账号 `caligod --media-config <absolute-json>` 可加载同一配置。`work_root` 必须是当前用户专用目录，其父目录须已存在。多账号 `caligo-manager` 从各账号私有目录读取可选的 `voice-media.json`，把工作目录固定到该账号的 `media-jobs`。工具配置由本机文件提供。

OneBot `get_record` 接受入站私聊语音的账号限定文件标识。`out_format` 省略时返回原始 Silk，配置 `silk_decoder` 后可选 `wav` 或 `ogg`。下载上限 128 KiB，转码的中间 PCM 与输出各最多 4 MiB，并受更低的本机配置限制。响应给出 Base64、实际字节数和固定文件名。缺配置或不支持的格式返回 1404，工具失败返回 1502，转码超时或停止返回 1501。

## 验收边界

`media_tool_lab` 使用自有 Rust fake process runner，覆盖固定格式、坏 hash、超时、超大 stdout/stderr、非零退出、停止、输入 regular-file 和 byte limit。fake 测试只证明本地边界合同；真实 ffmpeg 可在用户授权且本机已安装时做 readonly version/format 检查，不下载、不运行参考仓库脚本，不构成 QQ 现场或长期运行验收。
