//! Offline media utility entry point.
//!
//! This binary performs no QQ/native/OneBot work and never downloads tools.

use caligo_core::media_tool::{
    ContainerFormat, MediaTool, MediaToolConfig, StopToken, ThumbnailFormat,
};
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
static OUTPUT_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct Cli {
    config: PathBuf,
    operation: Operation,
    input: PathBuf,
    output_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
enum Operation {
    Probe,
    Pcm,
    Wav,
    Ogg,
    SilkWav,
    SilkOgg,
    Thumbnail,
}

#[derive(Serialize)]
struct ProbeOutput {
    operation: &'static str,
    duration_seconds: f64,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Serialize)]
struct ArtifactOutput {
    operation: &'static str,
    format: &'static str,
    path: PathBuf,
    bytes: usize,
}

fn usage() -> &'static str {
    "caligo-media --config <absolute-json> <probe|pcm|wav|ogg|silk-wav|silk-ogg|thumbnail> --input <absolute-file> [--output-root <absolute-dir>]"
}

fn parse_args() -> Result<Cli, String> {
    let mut args = std::env::args().skip(1);
    let mut config = None;
    let mut operation = None;
    let mut input = None;
    let mut output_root = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => config = Some(next_value(&mut args, "--config")?),
            "--input" => input = Some(next_value(&mut args, "--input")?),
            "--output-root" | "--outputRoot" => {
                output_root = Some(next_value(&mut args, "--output-root")?)
            }
            "probe" => operation = Some(Operation::Probe),
            "pcm" => operation = Some(Operation::Pcm),
            "wav" => operation = Some(Operation::Wav),
            "ogg" => operation = Some(Operation::Ogg),
            "silk-wav" => operation = Some(Operation::SilkWav),
            "silk-ogg" => operation = Some(Operation::SilkOgg),
            "thumbnail" => operation = Some(Operation::Thumbnail),
            "--help" | "-h" => return Err(usage().into()),
            unknown => return Err(format!("unknown argument {unknown}; usage: {}", usage())),
        }
    }
    Ok(Cli {
        config: absolute_arg(config, "--config")?,
        operation: operation.ok_or_else(|| format!("missing operation; usage: {}", usage()))?,
        input: absolute_arg(input, "--input")?,
        output_root: output_root
            .map(|value| absolute_path(value, "--output-root"))
            .transpose()?,
    })
}

fn next_value<I>(args: &mut I, flag: &str) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    args.next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn absolute_arg(value: Option<String>, flag: &str) -> Result<PathBuf, String> {
    absolute_path(value.ok_or_else(|| format!("missing {flag}"))?, flag)
}

fn absolute_path(value: String, label: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(format!("{label} must be absolute"));
    }
    Ok(path)
}

fn read_config(path: &Path) -> Result<MediaToolConfig, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("config metadata: {e}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("config must be a local regular file".into());
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(format!("config exceeds {MAX_CONFIG_BYTES} bytes"));
    }
    let file = File::open(path).map_err(|e| format!("config open: {e}"))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("config read: {e}"))?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(format!("config exceeds {MAX_CONFIG_BYTES} bytes"));
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("config JSON: {e}"))
}

fn validate_input(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("input metadata: {e}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("input must be a local regular file".into());
    }
    Ok(())
}

fn output_root(path: Option<PathBuf>) -> Result<PathBuf, String> {
    let path = path.ok_or_else(|| "this operation requires --output-root".to_string())?;
    fs::create_dir_all(&path).map_err(|e| format!("output root: {e}"))?;
    let metadata = fs::symlink_metadata(&path).map_err(|e| format!("output root metadata: {e}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("output root must be a regular directory".into());
    }
    Ok(path)
}

fn output_path(root: &Path, operation: &str, extension: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let id = OUTPUT_COUNTER.fetch_add(1, Ordering::Relaxed);
    root.join(format!("caligo-{operation}-{now:x}-{id:x}.{extension}"))
}

fn write_artifact(
    root: &Path,
    operation: &'static str,
    format: &'static str,
    bytes: &[u8],
) -> Result<ArtifactOutput, String> {
    let path = output_path(root, operation, format);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("output create: {e}"))?;
    use std::io::Write;
    file.write_all(bytes)
        .map_err(|e| format!("output write: {e}"))?;
    Ok(ArtifactOutput {
        operation,
        format,
        path,
        bytes: bytes.len(),
    })
}

fn run(cli: Cli) -> Result<serde_json::Value, String> {
    validate_input(&cli.input)?;
    let config = read_config(&cli.config)?;
    let tool = MediaTool::new(config).map_err(|e| e.to_string())?;
    let stop = StopToken::default();
    match cli.operation {
        Operation::Probe => {
            let result = tool.probe(&cli.input, &stop).map_err(|e| e.to_string())?;
            serde_json::to_value(ProbeOutput {
                operation: "probe",
                duration_seconds: result.duration_seconds,
                width: result.width,
                height: result.height,
            })
            .map_err(|e| e.to_string())
        }
        Operation::Pcm => {
            let root = output_root(cli.output_root)?;
            let artifact = tool
                .audio_to_pcm(&cli.input, &stop)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(write_artifact(&root, "pcm", "pcm", &artifact.bytes)?)
                .map_err(|e| e.to_string())
        }
        Operation::Wav | Operation::Ogg => {
            let root = output_root(cli.output_root)?;
            let metadata =
                fs::symlink_metadata(&cli.input).map_err(|e| format!("PCM input metadata: {e}"))?;
            let limit = tool.config().max_input_bytes;
            if metadata.len() > limit {
                return Err(format!("PCM input exceeds {limit} bytes"));
            }
            let mut pcm = Vec::with_capacity(metadata.len() as usize);
            File::open(&cli.input)
                .map_err(|e| format!("PCM input open: {e}"))?
                .take(limit + 1)
                .read_to_end(&mut pcm)
                .map_err(|e| format!("PCM input read: {e}"))?;
            if pcm.len() as u64 > limit {
                return Err(format!("PCM input exceeds {limit} bytes"));
            }
            let format = match cli.operation {
                Operation::Wav => ContainerFormat::Wav,
                Operation::Ogg => ContainerFormat::Ogg,
                _ => unreachable!(),
            };
            let artifact = tool
                .pcm_to_container(&pcm, format, &stop)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(write_artifact(
                &root,
                if matches!(format, ContainerFormat::Wav) {
                    "wav"
                } else {
                    "ogg"
                },
                artifact.format,
                &artifact.bytes,
            )?)
            .map_err(|e| e.to_string())
        }
        Operation::SilkWav | Operation::SilkOgg => {
            let (operation, format) = match cli.operation {
                Operation::SilkWav => ("silk-wav", ContainerFormat::Wav),
                _ => ("silk-ogg", ContainerFormat::Ogg),
            };
            let root = output_root(cli.output_root)?;
            let artifact = tool
                .silk_file_to_container(&cli.input, format, &stop)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(write_artifact(
                &root,
                operation,
                artifact.format,
                &artifact.bytes,
            )?)
            .map_err(|e| e.to_string())
        }
        Operation::Thumbnail => {
            let root = output_root(cli.output_root)?;
            let artifact = tool
                .thumbnail(&cli.input, ThumbnailFormat::Png, &stop)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(write_artifact(&root, "thumbnail", "png", &artifact.bytes)?)
                .map_err(|e| e.to_string())
        }
    }
}

fn main() {
    let result = parse_args().and_then(run);
    match result {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string(&value).unwrap_or_else(|_| "{\"status\":\"ok\"}".into())
            );
        }
        Err(error) => {
            eprintln!("caligo-media: {error}");
            std::process::exit(2);
        }
    }
}
