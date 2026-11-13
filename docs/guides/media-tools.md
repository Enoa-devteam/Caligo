# 媒体常规工具边界

## 范围

`caligo-core::media_tool` 是独立的离线媒体工具边界。它只负责在用户明确配置的本机 `ffmpeg`/`ffprobe` 上执行有限操作，不接入 QQ、OneBot、native adapter、消息收发或 NTSilk。固定参考合同为 `7cf025f` 的 `FFmpegExecAdapter`/`IFFmpegAdapter` 形状；本实现仅采用其媒体操作边界，不运行参考 TypeScript，也不引入 C++。

支持的操作是：

- `probe`：从 `ffprobe` JSON 读取 `format.duration` 和首个视频流的 `width`/`height`。
- `audio_to_pcm`：输出 24 kHz、单声道、signed 16-bit little-endian PCM。
- `pcm_to_container`：将上述 PCM 输出为固定枚举 `wav` 或 `ogg`。
- `thumbnail`：在固定 1 秒位置输出固定枚举 `png`，不造默认 `100x100` 或 `60 seconds` 降级结果。

## 配置合同

生产构造必须同时配置 `ffmpeg` 与 `ffprobe`：

- `executable`：绝对路径，必须是本地 regular file，不接受符号链接。
- `sha256`：64 位十六进制文件哈希；执行前逐字节校验，坏 hash fail closed。
- `version`：固定工具版本字符串，生产构造执行已核对 hash 的程序 `-version` 并验证输出包含该值。
- `license`：用户声明的 `name`、`source`，以及可选绝对 `path`。许可证路径/来源不执行认证、不联网。
- `work_root`：绝对路径。每个任务创建独立目录，任务结束自动 cleanup。
- `timeout_ms`、`max_stdout_stderr_bytes`、`max_input_bytes`、`max_output_bytes`：均必须为正数。

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
```

配置文件本身不执行下载或安装。CLI 只负责解析 JSON、调用 core 模块并报告结果；不会声称真实 QQ `get_record`、native 上传或 OneBot 媒体动作已经接通。NTSilk 不在本合同内，不能通过默认值或降级路径启用。

## 验收边界

`media_tool_lab` 使用自有 Rust fake process runner，覆盖固定格式、坏 hash、超时、超大 stdout/stderr、非零退出、停止、输入 regular-file 和 byte limit。fake 测试只证明本地边界合同；真实 ffmpeg 可在用户授权且本机已安装时做 readonly version/format 检查，不下载、不运行参考仓库脚本，不构成 QQ 现场或长期运行验收。
