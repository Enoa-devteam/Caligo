use caligo_core::media_tool::{
    os_process_runner, ContainerFormat, LicenseDeclaration, MediaError, MediaProcessRunner,
    MediaTool, MediaToolConfig, ProcessResult, StopToken, ThumbnailFormat, ToolDeclaration,
};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn temp_root(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("caligo-media-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn declaration(path: &Path, bytes: &[u8]) -> ToolDeclaration {
    fs::write(path, bytes).unwrap();
    let mut hash = Sha256::new();
    hash.update(bytes);
    ToolDeclaration {
        executable: path.to_path_buf(),
        sha256: format!("{:x}", hash.finalize()),
        version: "fake-1".into(),
        license: LicenseDeclaration {
            name: "test-license".into(),
            source: "local test fixture".into(),
            path: None,
        },
    }
}

fn config(root: &Path) -> MediaToolConfig {
    MediaToolConfig {
        ffmpeg: declaration(&root.join("fake-ffmpeg.exe"), b"fake ffmpeg executable"),
        ffprobe: declaration(&root.join("fake-ffprobe.exe"), b"fake ffprobe executable"),
        silk_decoder: None,
        work_root: root.join("jobs"),
        timeout_ms: 100,
        max_stdout_stderr_bytes: 1024,
        max_input_bytes: 1024 * 1024,
        max_output_bytes: 1024 * 1024,
    }
}

struct FakeRunner {
    mode: FakeMode,
}

enum FakeMode {
    Normal,
    Timeout,
    OutputLimit,
    ExitFailure,
    Stopped,
}

impl MediaProcessRunner for FakeRunner {
    fn run(
        &self,
        executable: &Path,
        args: &[OsString],
        cwd: &Path,
        _timeout: Duration,
        _output_limit: u64,
        _stop: &StopToken,
    ) -> Result<ProcessResult, MediaError> {
        assert!(executable.is_absolute());
        assert!(cwd.is_absolute());
        match self.mode {
            FakeMode::Timeout => return Err(MediaError::TimedOut),
            FakeMode::OutputLimit => return Err(MediaError::OutputLimit),
            FakeMode::ExitFailure => {
                return Ok(ProcessResult {
                    success: false,
                    code: Some(17),
                    stdout: Vec::new(),
                    stderr: b"fake failure".to_vec(),
                })
            }
            FakeMode::Stopped => return Err(MediaError::Stopped),
            FakeMode::Normal => {}
        }
        let output = args.last().expect("operation has an output/path argument");
        if args.first().is_some_and(|arg| arg == "decode") {
            assert_eq!(executable.file_name().unwrap(), "fake-silk.exe");
            assert_eq!(args[3], "--sample-rate");
            assert_eq!(args[4], "24000");
            assert!(fs::read(Path::new(&args[2]))
                .unwrap()
                .starts_with(b"\x02#!SILK_V3"));
        }
        let output = PathBuf::from(output);
        if output.extension().and_then(|value| value.to_str()) == Some("pcm") {
            fs::write(output, [0x00, 0x01, 0x02, 0x03]).unwrap();
            return Ok(ProcessResult {
                success: true,
                code: Some(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            });
        }
        if output.extension().and_then(|value| value.to_str()) == Some("wav") {
            fs::write(output, b"RIFFfakeWAVE").unwrap();
        } else if output.extension().and_then(|value| value.to_str()) == Some("ogg") {
            fs::write(output, b"OggSfake").unwrap();
        } else if output.extension().and_then(|value| value.to_str()) == Some("png") {
            fs::write(output, b"\x89PNG\r\n\x1a\nfake").unwrap();
        }
        Ok(ProcessResult {
            success: true,
            code: Some(0),
            stdout: br#"{"format":{"duration":"2.5"},"streams":[{"width":640,"height":360}]}"#
                .to_vec(),
            stderr: Vec::new(),
        })
    }
}

fn tool(root: &Path, mode: FakeMode) -> MediaTool {
    MediaTool::with_runner_for_tests(config(root), Arc::new(FakeRunner { mode })).unwrap()
}

#[test]
fn regular_operations_return_fixed_formats_and_cleanup_jobs() {
    let root = temp_root("formats");
    let input = root.join("input.local");
    fs::write(&input, b"regular local fixture").unwrap();
    let media = tool(&root, FakeMode::Normal);
    let stop = StopToken::default();

    let probe = media.probe(&input, &stop).unwrap();
    assert_eq!(probe.duration_seconds, 2.5);
    assert_eq!((probe.width, probe.height), (Some(640), Some(360)));

    let pcm = media.audio_to_pcm(&input, &stop).unwrap();
    assert_eq!(pcm.format, "s16le-24khz-mono");
    assert_eq!(pcm.bytes, vec![0, 1, 2, 3]);

    let wav = media
        .pcm_to_container(&pcm.bytes, ContainerFormat::Wav, &stop)
        .unwrap();
    assert_eq!(wav.format, "wav");
    assert_eq!(&wav.bytes[..4], b"RIFF");

    let ogg = media
        .pcm_to_container(&pcm.bytes, ContainerFormat::Ogg, &stop)
        .unwrap();
    assert_eq!(ogg.format, "ogg");
    assert_eq!(&ogg.bytes[..4], b"OggS");

    let thumbnail = media
        .thumbnail(&input, ThumbnailFormat::Png, &stop)
        .unwrap();
    assert_eq!(thumbnail.format, "png");
    assert_eq!(&thumbnail.bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert!(fs::read_dir(root.join("jobs"))
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn bad_hash_is_rejected_before_media_execution() {
    let root = temp_root("hash");
    let mut configuration = config(&root);
    configuration.ffmpeg.sha256 = "00".repeat(32);
    let error = match MediaTool::with_runner_for_tests(
        configuration,
        Arc::new(FakeRunner {
            mode: FakeMode::Normal,
        }),
    ) {
        Ok(_) => panic!("bad hash unexpectedly accepted"),
        Err(error) => error,
    };
    assert!(matches!(error, MediaError::ToolHashMismatch { .. }));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn silk_conversion_requires_a_configured_decoder_and_cleans_private_jobs() {
    let root = temp_root("silk");
    let mut configuration = config(&root);
    let runner = Arc::new(FakeRunner {
        mode: FakeMode::Normal,
    });
    let stop = StopToken::default();
    let voice = b"\x02#!SILK_V3\x02\x00\x11\x22";
    let media = MediaTool::with_runner_for_tests(configuration.clone(), runner.clone()).unwrap();
    assert!(matches!(
        media.silk_to_container(voice, ContainerFormat::Wav, &stop),
        Err(MediaError::ToolUnavailable(_))
    ));
    let decoder = root.join("fake-silk.exe");
    configuration.silk_decoder = Some(declaration(&decoder, b"fixture decoder"));
    let media = MediaTool::with_runner_for_tests(configuration, runner).unwrap();
    assert!(matches!(
        media.silk_to_container(b"#!SILK_V3", ContainerFormat::Wav, &stop),
        Err(MediaError::InvalidInput(_))
    ));
    for format in [ContainerFormat::Wav, ContainerFormat::Ogg] {
        let converted = media.silk_to_container(voice, format, &stop).unwrap();
        assert_eq!(
            &converted.bytes[..4],
            if format == ContainerFormat::Wav {
                b"RIFF"
            } else {
                b"OggS"
            }
        );
    }
    assert!(fs::read_dir(root.join("jobs")).unwrap().next().is_none());
    fs::write(&decoder, b"changed decoder").unwrap();
    assert!(matches!(
        media.silk_to_container(voice, ContainerFormat::Wav, &stop),
        Err(MediaError::ToolHashMismatch { .. })
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn timeout_output_limit_and_exit_failure_are_preserved() {
    let root = temp_root("failures");
    let input = root.join("input.local");
    fs::write(&input, b"regular local fixture").unwrap();
    let stop = StopToken::default();

    let timeout = tool(&root, FakeMode::Timeout)
        .probe(&input, &stop)
        .unwrap_err();
    assert!(matches!(timeout, MediaError::TimedOut));
    let output = tool(&root, FakeMode::OutputLimit)
        .probe(&input, &stop)
        .unwrap_err();
    assert!(matches!(output, MediaError::OutputLimit));
    let failed = tool(&root, FakeMode::ExitFailure)
        .probe(&input, &stop)
        .unwrap_err();
    assert!(matches!(
        failed,
        MediaError::ProcessFailed { code: Some(17), .. }
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn input_must_be_absolute_regular_file_and_pcm_is_bounded() {
    let root = temp_root("input");
    let media = tool(&root, FakeMode::Normal);
    let stop = StopToken::default();
    let relative = Path::new("relative.input");
    assert!(matches!(
        media.probe(relative, &stop),
        Err(MediaError::InvalidConfig(_))
    ));
    let directory = root.join("directory");
    fs::create_dir(&directory).unwrap();
    assert!(matches!(
        media.probe(&directory, &stop),
        Err(MediaError::InvalidInput(_))
    ));
    assert!(matches!(
        media.pcm_to_container(&[1], ContainerFormat::Wav, &stop),
        Err(MediaError::InvalidInput(_))
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn stop_token_is_observable_and_does_not_create_an_output() {
    let root = temp_root("stop");
    let input = root.join("input.local");
    fs::write(&input, b"regular local fixture").unwrap();
    let stop = StopToken::default();
    stop.stop();
    assert!(stop.is_stopped());
    let error = tool(&root, FakeMode::Stopped)
        .probe(&input, &stop)
        .unwrap_err();
    assert!(matches!(error, MediaError::Stopped));
    let _ = fs::remove_dir_all(root);
}

#[test]
#[ignore = "runs the current test executable as a child fixture"]
fn process_child_fixture() {
    match std::env::args().find(|arg| arg == "--media-child-timeout") {
        Some(_) => std::thread::sleep(Duration::from_secs(30)),
        None => {
            let count = std::env::args()
                .find_map(|arg| {
                    arg.strip_prefix("--media-child-output=")
                        .and_then(|v| v.parse::<usize>().ok())
                })
                .unwrap_or(0);
            if count > 0 {
                print!("{}", "x".repeat(count));
            }
        }
    }
}

#[test]
fn real_runner_kills_timed_out_child_and_caps_combined_output() {
    let executable = std::env::current_exe().unwrap();
    let runner = os_process_runner();
    let stop = StopToken::default();
    let cwd = temp_root("child-cwd");
    let timeout = runner
        .run(
            &executable,
            &[
                OsString::from("--ignored"),
                OsString::from("--exact"),
                OsString::from("process_child_fixture"),
                OsString::from("--nocapture"),
                OsString::from("--"),
                OsString::from("--media-child-timeout"),
            ],
            &cwd,
            Duration::from_millis(100),
            1024,
            &stop,
        )
        .unwrap_err();
    assert!(matches!(timeout, MediaError::TimedOut));

    let output = runner
        .run(
            &executable,
            &[
                OsString::from("--ignored"),
                OsString::from("--exact"),
                OsString::from("process_child_fixture"),
                OsString::from("--nocapture"),
                OsString::from("--"),
                OsString::from("--media-child-output=4096"),
            ],
            &cwd,
            Duration::from_secs(2),
            512,
            &stop,
        )
        .unwrap_err();
    assert!(matches!(output, MediaError::OutputLimit));
    let _ = fs::remove_dir_all(cwd);
}
