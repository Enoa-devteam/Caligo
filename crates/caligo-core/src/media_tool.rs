//! Controlled offline media operations backed by explicitly configured tools.
//! Independent of QQ and native adapters.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_OUTPUT_LIMIT: u64 = 4 * 1024 * 1024;
const DEFAULT_INPUT_LIMIT: u64 = 512 * 1024 * 1024;
const DEFAULT_FILE_LIMIT: u64 = 512 * 1024 * 1024;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const MAX_RECORD_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;
const HASH_READ_CHUNK: usize = 1024 * 1024;
static JOB_COUNTER: AtomicU64 = AtomicU64::new(0);

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}
fn default_output_limit() -> u64 {
    DEFAULT_OUTPUT_LIMIT
}
fn default_input_limit() -> u64 {
    DEFAULT_INPUT_LIMIT
}
fn default_file_limit() -> u64 {
    DEFAULT_FILE_LIMIT
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LicenseDeclaration {
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDeclaration {
    pub executable: PathBuf,
    pub sha256: String,
    pub version: String,
    pub license: LicenseDeclaration,
}

impl ToolDeclaration {
    fn validate_metadata(&self, role: ToolRole) -> Result<(), MediaError> {
        require_absolute(&self.executable, "tool executable")?;
        if self.version.trim().is_empty()
            || self.license.name.trim().is_empty()
            || self.license.source.trim().is_empty()
        {
            return Err(MediaError::InvalidConfig(format!(
                "{} version and license declarations must not be empty",
                role.as_str()
            )));
        }
        if let Some(path) = &self.license.path {
            require_absolute(path, "license path")?;
        }
        let hash = self.sha256.trim();
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(MediaError::InvalidConfig(format!(
                "{} SHA-256 must be 64 hexadecimal characters",
                role.as_str()
            )));
        }
        Ok(())
    }

    pub fn verify(&self, role: ToolRole) -> Result<(), MediaError> {
        self.validate_metadata(role)?;
        let metadata = fs::symlink_metadata(&self.executable).map_err(|e| {
            MediaError::ToolUnavailable(format!(
                "{} executable {}: {e}",
                role.as_str(),
                self.executable.display()
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(MediaError::ToolUnavailable(format!(
                "{} executable is not a regular file: {}",
                role.as_str(),
                self.executable.display()
            )));
        }
        if metadata.len() > MAX_EXECUTABLE_BYTES {
            return Err(MediaError::FileTooLarge {
                path: self.executable.clone(),
                limit: MAX_EXECUTABLE_BYTES,
            });
        }
        let mut file = File::open(&self.executable).map_err(MediaError::Io)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; HASH_READ_CHUNK];
        loop {
            let count = file.read(&mut buffer).map_err(MediaError::Io)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        let actual = format!("{:x}", hasher.finalize());
        if !actual.eq_ignore_ascii_case(self.sha256.trim()) {
            return Err(MediaError::ToolHashMismatch {
                role,
                expected: self.sha256.clone(),
                actual,
            });
        }
        if let Some(path) = &self.license.path {
            let metadata = fs::symlink_metadata(path).map_err(|e| {
                MediaError::InvalidConfig(format!("license path {}: {e}", path.display()))
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(MediaError::InvalidConfig(format!(
                    "license path is not a regular file: {}",
                    path.display()
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRole {
    Ffmpeg,
    Ffprobe,
    SilkDecoder,
}
impl ToolRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::Ffprobe => "ffprobe",
            Self::SilkDecoder => "silk decoder",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaToolConfig {
    pub ffmpeg: ToolDeclaration,
    pub ffprobe: ToolDeclaration,
    #[serde(default)]
    pub silk_decoder: Option<ToolDeclaration>,
    pub work_root: PathBuf,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_output_limit")]
    pub max_stdout_stderr_bytes: u64,
    #[serde(default = "default_input_limit")]
    pub max_input_bytes: u64,
    #[serde(default = "default_file_limit")]
    pub max_output_bytes: u64,
}
impl MediaToolConfig {
    pub fn read_from(path: &Path) -> Result<Self, MediaError> {
        require_absolute(path, "media config path")?;
        let bytes = read_bounded(path, MAX_CONFIG_BYTES)?;
        serde_json::from_slice(&bytes)
            .map_err(|error| MediaError::InvalidConfig(format!("config JSON: {error}")))
    }

    pub fn validate(&self) -> Result<(), MediaError> {
        require_absolute(&self.work_root, "media work root")?;
        if self.timeout_ms == 0
            || self.max_stdout_stderr_bytes == 0
            || self.max_input_bytes == 0
            || self.max_output_bytes == 0
        {
            return Err(MediaError::InvalidConfig(
                "timeout and all byte limits must be positive".into(),
            ));
        }
        self.ffmpeg.validate_metadata(ToolRole::Ffmpeg)?;
        self.ffprobe.validate_metadata(ToolRole::Ffprobe)?;
        if let Some(decoder) = &self.silk_decoder {
            decoder.validate_metadata(ToolRole::SilkDecoder)?;
        }
        Ok(())
    }
    pub fn verify_tools(&self) -> Result<(), MediaError> {
        self.validate()?;
        self.ffmpeg.verify(ToolRole::Ffmpeg)?;
        self.ffprobe.verify(ToolRole::Ffprobe)?;
        if let Some(decoder) = &self.silk_decoder {
            decoder.verify(ToolRole::SilkDecoder)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerFormat {
    Wav,
    Ogg,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThumbnailFormat {
    Png,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeResult {
    pub duration_seconds: f64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaArtifact {
    pub format: &'static str,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct StopToken(Arc<AtomicBool>);
impl StopToken {
    pub(crate) fn from_flag(flag: Arc<AtomicBool>) -> Self {
        Self(flag)
    }

    pub fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
pub enum MediaError {
    InvalidConfig(String),
    InvalidInput(String),
    ToolUnavailable(String),
    ToolHashMismatch {
        role: ToolRole,
        expected: String,
        actual: String,
    },
    ToolVersionMismatch {
        role: ToolRole,
        expected: String,
        actual: String,
    },
    Io(io::Error),
    Spawn(String),
    ProcessFailed {
        role: ToolRole,
        code: Option<i32>,
        stderr: Vec<u8>,
    },
    TimedOut,
    Stopped,
    OutputLimit,
    FileTooLarge {
        path: PathBuf,
        limit: u64,
    },
    InvalidOutput(String),
    InvalidProbe(String),
}
impl std::fmt::Display for MediaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(s) => write!(f, "invalid media config: {s}"),
            Self::InvalidInput(s) => write!(f, "invalid media input: {s}"),
            Self::ToolUnavailable(s) => write!(f, "media tool unavailable: {s}"),
            Self::ToolHashMismatch {
                role,
                expected,
                actual,
            } => write!(
                f,
                "{} SHA-256 mismatch: expected {expected}, got {actual}",
                role.as_str()
            ),
            Self::ToolVersionMismatch {
                role,
                expected,
                actual,
            } => write!(
                f,
                "{} version mismatch: expected declaration {expected:?}, output was {actual:?}",
                role.as_str()
            ),
            Self::Io(e) => write!(f, "media I/O: {e}"),
            Self::Spawn(s) => write!(f, "media process spawn: {s}"),
            Self::ProcessFailed { role, code, .. } => {
                write!(f, "{} exited unsuccessfully ({code:?})", role.as_str())
            }
            Self::TimedOut => f.write_str("media process timed out"),
            Self::Stopped => f.write_str("media process stopped"),
            Self::OutputLimit => f.write_str("media stdout/stderr limit exceeded"),
            Self::FileTooLarge { path, limit } => {
                write!(f, "file {} exceeds {limit} bytes", path.display())
            }
            Self::InvalidOutput(s) => write!(f, "invalid media output: {s}"),
            Self::InvalidProbe(s) => write!(f, "invalid ffprobe JSON: {s}"),
        }
    }
}
impl std::error::Error for MediaError {}
impl From<io::Error> for MediaError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub struct ProcessResult {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
#[doc(hidden)]
pub trait MediaProcessRunner: Send + Sync {
    fn run(
        &self,
        executable: &Path,
        args: &[OsString],
        cwd: &Path,
        timeout: Duration,
        output_limit: u64,
        stop: &StopToken,
    ) -> Result<ProcessResult, MediaError>;
}
#[doc(hidden)]
pub struct OsProcessRunner;
#[doc(hidden)]
pub fn os_process_runner() -> OsProcessRunner {
    OsProcessRunner
}

impl MediaProcessRunner for OsProcessRunner {
    fn run(
        &self,
        executable: &Path,
        args: &[OsString],
        cwd: &Path,
        timeout: Duration,
        output_limit: u64,
        stop: &StopToken,
    ) -> Result<ProcessResult, MediaError> {
        let mut command = Command::new(executable);
        command
            .current_dir(cwd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| MediaError::Spawn(format!("{}: {e}", executable.display())))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MediaError::Spawn("stdout pipe unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| MediaError::Spawn("stderr pipe unavailable".into()))?;
        let total = Arc::new(AtomicU64::new(0));
        let exceeded = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let out_thread = spawn_reader(
            stdout,
            output_limit,
            Arc::clone(&total),
            Arc::clone(&exceeded),
            Arc::clone(&failed),
        );
        let err_thread = spawn_reader(
            stderr,
            output_limit,
            total,
            Arc::clone(&exceeded),
            Arc::clone(&failed),
        );
        let started = Instant::now();
        let status = loop {
            if exceeded.load(Ordering::Acquire) {
                kill_and_wait(&mut child);
                let _ = join_reader(out_thread);
                let _ = join_reader(err_thread);
                return Err(MediaError::OutputLimit);
            }
            if stop.is_stopped() {
                kill_and_wait(&mut child);
                let _ = join_reader(out_thread);
                let _ = join_reader(err_thread);
                return Err(MediaError::Stopped);
            }
            if started.elapsed() >= timeout {
                kill_and_wait(&mut child);
                let _ = join_reader(out_thread);
                let _ = join_reader(err_thread);
                return Err(MediaError::TimedOut);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(error) => {
                    kill_and_wait(&mut child);
                    let _ = join_reader(out_thread);
                    let _ = join_reader(err_thread);
                    return Err(MediaError::Io(error));
                }
            }
        };
        let stdout = join_reader(out_thread).map_err(MediaError::Io)?;
        let stderr = join_reader(err_thread).map_err(MediaError::Io)?;
        if exceeded.load(Ordering::Acquire) {
            return Err(MediaError::OutputLimit);
        }
        if failed.load(Ordering::Acquire) {
            return Err(MediaError::Io(io::Error::new(
                io::ErrorKind::Other,
                "failed to read media process output",
            )));
        }
        Ok(ProcessResult {
            success: status.success(),
            code: status.code(),
            stdout,
            stderr,
        })
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    limit: u64,
    total: Arc<AtomicU64>,
    exceeded: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(n) => n,
                Err(e) => {
                    failed.store(true, Ordering::Release);
                    return Err(e);
                }
            };
            if count == 0 {
                return Ok(output);
            }
            let previous = total.fetch_add(count as u64, Ordering::AcqRel);
            if previous.saturating_add(count as u64) > limit {
                exceeded.store(true, Ordering::Release);
                return Ok(output);
            }
            output.extend_from_slice(&buffer[..count]);
        }
    })
}
fn join_reader(handle: thread::JoinHandle<io::Result<Vec<u8>>>) -> io::Result<Vec<u8>> {
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::Other,
            "media output reader panicked",
        )),
    }
}
fn kill_and_wait(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub struct MediaTool {
    config: MediaToolConfig,
    runner: Arc<dyn MediaProcessRunner>,
}
impl MediaTool {
    pub fn for_voice_records(mut config: MediaToolConfig) -> Result<Self, MediaError> {
        config.validate()?;
        config.max_input_bytes = config.max_input_bytes.min(MAX_RECORD_OUTPUT_BYTES);
        config.max_output_bytes = config.max_output_bytes.min(MAX_RECORD_OUTPUT_BYTES);
        if let Some(parent) = config.work_root.parent() {
            crate::account_manager::reject_reparse_chain(parent)
                .map_err(|error| MediaError::InvalidConfig(format!("voice work root parent: {error}")))?;
        }
        crate::account_manager::ensure_private_dir(&config.work_root).map_err(|error| {
            MediaError::InvalidConfig(format!("private voice work root: {error}"))
        })?;
        Self::new(config)
    }

    pub fn new(config: MediaToolConfig) -> Result<Self, MediaError> {
        config.verify_tools()?;
        let runner: Arc<dyn MediaProcessRunner> = Arc::new(OsProcessRunner);
        verify_runtime_version(
            &config.ffmpeg,
            ToolRole::Ffmpeg,
            &runner,
            &config.work_root,
            config.timeout_ms,
            config.max_stdout_stderr_bytes,
        )?;
        verify_runtime_version(
            &config.ffprobe,
            ToolRole::Ffprobe,
            &runner,
            &config.work_root,
            config.timeout_ms,
            config.max_stdout_stderr_bytes,
        )?;
        if let Some(decoder) = &config.silk_decoder {
            verify_runtime_version(
                decoder,
                ToolRole::SilkDecoder,
                &runner,
                &config.work_root,
                config.timeout_ms,
                config.max_stdout_stderr_bytes,
            )?;
        }
        Ok(Self { config, runner })
    }
    #[doc(hidden)]
    pub fn with_runner_for_tests(
        config: MediaToolConfig,
        runner: Arc<dyn MediaProcessRunner>,
    ) -> Result<Self, MediaError> {
        config.verify_tools()?;
        Ok(Self { config, runner })
    }
    pub fn config(&self) -> &MediaToolConfig {
        &self.config
    }

    pub fn probe(&self, input: &Path, stop: &StopToken) -> Result<ProbeResult, MediaError> {
        let input = self.prepare_input(input)?;
        let job = JobDirectory::new(&self.config.work_root)?;
        let args = vec![
            OsString::from("-nostdin"),
            OsString::from("-protocol_whitelist"),
            OsString::from("file,pipe"),
            OsString::from("-v"),
            OsString::from("error"),
            OsString::from("-of"),
            OsString::from("json"),
            OsString::from("-show_entries"),
            OsString::from("format=duration:stream=width,height"),
            OsString::from("-select_streams"),
            OsString::from("v:0"),
            input.into_os_string(),
        ];
        let result = self.run(ToolRole::Ffprobe, &args, &job.path, stop)?;
        let value: ProbeJson = serde_json::from_slice(&result.stdout)
            .map_err(|e| MediaError::InvalidProbe(e.to_string()))?;
        let duration = value
            .format
            .and_then(|f| f.duration)
            .ok_or_else(|| MediaError::InvalidProbe("missing format.duration".into()))?;
        let duration_seconds = duration
            .parse::<f64>()
            .map_err(|_| MediaError::InvalidProbe("duration is not a number".into()))?;
        if !duration_seconds.is_finite() || duration_seconds < 0.0 {
            return Err(MediaError::InvalidProbe("duration is invalid".into()));
        }
        let stream = value.streams.into_iter().next();
        let width = stream.as_ref().and_then(|s| s.width);
        let height = stream.as_ref().and_then(|s| s.height);
        if width == Some(0) || height == Some(0) {
            return Err(MediaError::InvalidProbe(
                "dimensions must be positive".into(),
            ));
        }
        Ok(ProbeResult {
            duration_seconds,
            width,
            height,
        })
    }

    pub fn audio_to_pcm(
        &self,
        input: &Path,
        stop: &StopToken,
    ) -> Result<MediaArtifact, MediaError> {
        let input = self.prepare_input(input)?;
        let job = JobDirectory::new(&self.config.work_root)?;
        let output = job.path.join("audio.pcm");
        let args = vec![
            OsString::from("-nostdin"),
            OsString::from("-protocol_whitelist"),
            OsString::from("file,pipe"),
            OsString::from("-v"),
            OsString::from("error"),
            OsString::from("-i"),
            input.into_os_string(),
            OsString::from("-vn"),
            OsString::from("-ac"),
            OsString::from("1"),
            OsString::from("-ar"),
            OsString::from("24000"),
            OsString::from("-f"),
            OsString::from("s16le"),
            OsString::from("-y"),
            output.clone().into_os_string(),
        ];
        self.run(ToolRole::Ffmpeg, &args, &job.path, stop)?;
        let bytes = read_bounded(&output, self.config.max_output_bytes)?;
        if bytes.is_empty() || bytes.len() % 2 != 0 {
            return Err(MediaError::InvalidOutput(
                "PCM must be non-empty and contain complete s16le samples".into(),
            ));
        }
        Ok(MediaArtifact {
            format: "s16le-24khz-mono",
            bytes,
        })
    }

    /// Decode one local SILK v3 file and wrap the mono 24 kHz PCM as WAV or Ogg.
    pub fn silk_file_to_container(
        &self,
        input: &Path,
        format: ContainerFormat,
        stop: &StopToken,
    ) -> Result<MediaArtifact, MediaError> {
        let input = self.prepare_input(input)?;
        let bytes = read_bounded(&input, self.config.max_input_bytes)?;
        self.silk_to_container(&bytes, format, stop)
    }

    pub fn silk_to_container(
        &self,
        silk: &[u8],
        format: ContainerFormat,
        stop: &StopToken,
    ) -> Result<MediaArtifact, MediaError> {
        if stop.is_stopped() {
            return Err(MediaError::Stopped);
        }
        if silk.len() as u64 > self.config.max_input_bytes {
            return Err(MediaError::FileTooLarge {
                path: PathBuf::from("<silk>"),
                limit: self.config.max_input_bytes,
            });
        }
        let header = silk.strip_prefix(b"\x02").unwrap_or(silk);
        if !header.starts_with(b"#!SILK_V3") || header.len() <= 11 {
            return Err(MediaError::InvalidInput(
                "expected SILK v3 with packet data".into(),
            ));
        }
        let decoder =
            self.config.silk_decoder.as_ref().ok_or_else(|| {
                MediaError::ToolUnavailable("SILK decoder is not configured".into())
            })?;
        // A configuration can outlive its executable. Check the decoder again
        // before giving it untrusted media.
        decoder.verify(ToolRole::SilkDecoder)?;
        let job = JobDirectory::new(&self.config.work_root)?;
        let input = job.path.join("input.silk");
        let output = job.path.join("decoded.pcm");
        write_private_file(&input, silk)?;
        let args = [
            "decode".into(),
            "-i".into(),
            input.into_os_string(),
            "--sample-rate".into(),
            "24000".into(),
            "-o".into(),
            output.clone().into_os_string(),
        ];
        self.run(ToolRole::SilkDecoder, &args, &job.path, stop)?;
        let pcm = read_bounded(
            &output,
            self.config
                .max_input_bytes
                .min(self.config.max_output_bytes),
        )?;
        if pcm.is_empty() || pcm.len() % 2 != 0 {
            return Err(MediaError::InvalidOutput(
                "decoder returned incomplete s16le PCM".into(),
            ));
        }
        if stop.is_stopped() {
            return Err(MediaError::Stopped);
        }
        self.config.ffmpeg.verify(ToolRole::Ffmpeg)?;
        self.pcm_to_container(&pcm, format, stop)
    }

    pub fn pcm_to_container(
        &self,
        pcm: &[u8],
        format: ContainerFormat,
        stop: &StopToken,
    ) -> Result<MediaArtifact, MediaError> {
        if pcm.is_empty() || pcm.len() % 2 != 0 {
            return Err(MediaError::InvalidInput(
                "PCM must be non-empty and contain complete s16le samples".into(),
            ));
        }
        if pcm.len() as u64 > self.config.max_input_bytes {
            return Err(MediaError::FileTooLarge {
                path: PathBuf::from("<pcm>"),
                limit: self.config.max_input_bytes,
            });
        }
        let job = JobDirectory::new(&self.config.work_root)?;
        let input = job.path.join("input.pcm");
        let output = job.path.join(match format {
            ContainerFormat::Wav => "output.wav",
            ContainerFormat::Ogg => "output.ogg",
        });
        write_private_file(&input, pcm)?;
        let mut args = vec![
            OsString::from("-nostdin"),
            OsString::from("-protocol_whitelist"),
            OsString::from("file,pipe"),
            OsString::from("-v"),
            OsString::from("error"),
            OsString::from("-f"),
            OsString::from("s16le"),
            OsString::from("-ar"),
            OsString::from("24000"),
            OsString::from("-ac"),
            OsString::from("1"),
            OsString::from("-i"),
            input.into_os_string(),
        ];
        match format {
            ContainerFormat::Wav => {
                args.extend([OsString::from("-c:a"), OsString::from("pcm_s16le")])
            }
            ContainerFormat::Ogg => args.extend([
                OsString::from("-c:a"),
                OsString::from("libopus"),
                OsString::from("-b:a"),
                OsString::from("32k"),
            ]),
        }
        args.extend([OsString::from("-y"), output.clone().into_os_string()]);
        self.run(ToolRole::Ffmpeg, &args, &job.path, stop)?;
        let bytes = read_bounded(&output, self.config.max_output_bytes)?;
        let valid = match format {
            ContainerFormat::Wav => {
                bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE"
            }
            ContainerFormat::Ogg => bytes.len() >= 4 && &bytes[..4] == b"OggS",
        };
        if !valid {
            return Err(MediaError::InvalidOutput(format!(
                "invalid {:?} container signature",
                format
            )));
        }
        Ok(MediaArtifact {
            format: match format {
                ContainerFormat::Wav => "wav",
                ContainerFormat::Ogg => "ogg",
            },
            bytes,
        })
    }

    pub fn thumbnail(
        &self,
        input: &Path,
        format: ThumbnailFormat,
        stop: &StopToken,
    ) -> Result<MediaArtifact, MediaError> {
        let input = self.prepare_input(input)?;
        let job = JobDirectory::new(&self.config.work_root)?;
        let output = job.path.join("thumbnail.png");
        match format {
            ThumbnailFormat::Png => {
                let args = vec![
                    OsString::from("-nostdin"),
                    OsString::from("-protocol_whitelist"),
                    OsString::from("file,pipe"),
                    OsString::from("-v"),
                    OsString::from("error"),
                    OsString::from("-ss"),
                    OsString::from("1"),
                    OsString::from("-i"),
                    input.into_os_string(),
                    OsString::from("-frames:v"),
                    OsString::from("1"),
                    OsString::from("-f"),
                    OsString::from("image2"),
                    OsString::from("-c:v"),
                    OsString::from("png"),
                    OsString::from("-y"),
                    output.clone().into_os_string(),
                ];
                self.run(ToolRole::Ffmpeg, &args, &job.path, stop)?;
            }
        }
        let bytes = read_bounded(&output, self.config.max_output_bytes)?;
        if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
            return Err(MediaError::InvalidOutput("thumbnail is not PNG".into()));
        }
        Ok(MediaArtifact {
            format: "png",
            bytes,
        })
    }

    fn prepare_input(&self, input: &Path) -> Result<PathBuf, MediaError> {
        require_absolute(input, "input")?;
        let metadata = fs::symlink_metadata(input)
            .map_err(|e| MediaError::InvalidInput(format!("input {}: {e}", input.display())))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(MediaError::InvalidInput(format!(
                "input must be a local regular file: {}",
                input.display()
            )));
        }
        if metadata.len() > self.config.max_input_bytes {
            return Err(MediaError::FileTooLarge {
                path: input.to_path_buf(),
                limit: self.config.max_input_bytes,
            });
        }
        if input
            .file_name()
            .and_then(|n| n.to_str())
            .is_none_or(|n| n.is_empty() || n == "." || n == "..")
        {
            return Err(MediaError::InvalidInput("input basename is invalid".into()));
        }
        Ok(input.to_path_buf())
    }
    fn run(
        &self,
        role: ToolRole,
        args: &[OsString],
        cwd: &Path,
        stop: &StopToken,
    ) -> Result<ProcessResult, MediaError> {
        if stop.is_stopped() {
            return Err(MediaError::Stopped);
        }
        let declaration = match role {
            ToolRole::Ffmpeg => &self.config.ffmpeg,
            ToolRole::Ffprobe => &self.config.ffprobe,
            ToolRole::SilkDecoder => self.config.silk_decoder.as_ref().ok_or_else(|| {
                MediaError::ToolUnavailable("SILK decoder is not configured".into())
            })?,
        };
        let result = self.runner.run(
            &declaration.executable,
            args,
            cwd,
            Duration::from_millis(self.config.timeout_ms),
            self.config.max_stdout_stderr_bytes,
            stop,
        )?;
        if !result.success {
            return Err(MediaError::ProcessFailed {
                role,
                code: result.code,
                stderr: result.stderr,
            });
        }
        Ok(result)
    }
}

fn verify_runtime_version(
    declaration: &ToolDeclaration,
    role: ToolRole,
    runner: &Arc<dyn MediaProcessRunner>,
    work_root: &Path,
    timeout_ms: u64,
    output_limit: u64,
) -> Result<(), MediaError> {
    let job = JobDirectory::new(work_root)?;
    let result = runner.run(
        &declaration.executable,
        &[OsString::from(match role {
            ToolRole::SilkDecoder => "--version",
            _ => "-version",
        })],
        &job.path,
        Duration::from_millis(timeout_ms),
        output_limit,
        &StopToken::default(),
    )?;
    if !result.success {
        return Err(MediaError::ProcessFailed {
            role,
            code: result.code,
            stderr: result.stderr,
        });
    }
    let mut text = result.stdout;
    text.extend_from_slice(&result.stderr);
    let text = String::from_utf8_lossy(&text);
    if !text.contains(declaration.version.trim()) {
        return Err(MediaError::ToolVersionMismatch {
            role,
            expected: declaration.version.clone(),
            actual: text.into_owned(),
        });
    }
    Ok(())
}

#[derive(Deserialize)]
struct ProbeJson {
    #[serde(default)]
    format: Option<ProbeFormat>,
    #[serde(default)]
    streams: Vec<ProbeStream>,
}
#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}
#[derive(Deserialize)]
struct ProbeStream {
    width: Option<u32>,
    height: Option<u32>,
}

struct JobDirectory {
    path: PathBuf,
}
impl JobDirectory {
    fn new(root: &Path) -> Result<Self, MediaError> {
        require_absolute(root, "media work root")?;
        fs::create_dir_all(root).map_err(MediaError::Io)?;
        for _ in 0..16 {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let id = JOB_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("job-{now:x}-{id:x}"));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(MediaError::Io(e)),
            }
        }
        Err(MediaError::InvalidConfig(
            "could not allocate unique job directory".into(),
        ))
    }
}
impl Drop for JobDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), MediaError> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(MediaError::Io)?;
    f.write_all(bytes).map_err(MediaError::Io)
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, MediaError> {
    let metadata = fs::symlink_metadata(path).map_err(MediaError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(MediaError::InvalidOutput(format!(
            "output is not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > limit {
        return Err(MediaError::FileTooLarge {
            path: path.to_path_buf(),
            limit,
        });
    }
    let f = File::open(path).map_err(MediaError::Io)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    f.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(MediaError::Io)?;
    if bytes.len() as u64 > limit {
        return Err(MediaError::FileTooLarge {
            path: path.to_path_buf(),
            limit,
        });
    }
    Ok(bytes)
}
fn require_absolute(path: &Path, label: &str) -> Result<(), MediaError> {
    if !path.is_absolute() {
        return Err(MediaError::InvalidConfig(format!(
            "{label} must be absolute: {}",
            path.display()
        )));
    }
    Ok(())
}
