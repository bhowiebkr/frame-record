use std::{
    ffi::OsString,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Instant,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Codec {
    #[default]
    H264,
    H265,
}

impl Codec {
    pub fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::H265 => "H.265",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodingOptions {
    pub fps: u16,
    pub compression: u8,
}

impl Default for EncodingOptions {
    fn default() -> Self {
        Self {
            fps: 60,
            compression: 23,
        }
    }
}

impl EncodingOptions {
    fn validate(self) -> Result<(), String> {
        if !(1..=240).contains(&self.fps) {
            return Err("Frame rate must be between 1 and 240 FPS.".into());
        }
        if self.compression > 51 {
            return Err("Compression must be between 0 and 51.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    pub name: String,
    pub id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW: FFmpeg has no console UI.
    command
}

pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FRAME_RECORD_FFMPEG").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    #[cfg(debug_assertions)]
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    for root in roots {
        for relative in [
            "ffmpeg.exe",
            "bin/ffmpeg.exe",
            "tools/ffmpeg/bin/ffmpeg.exe",
        ] {
            let candidate = root.join(relative);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("ffmpeg.exe"))
            .find(|candidate| candidate.is_file())
    })
}

pub fn list_audio_devices(ffmpeg: &Path) -> Result<Vec<AudioDevice>, String> {
    // FFmpeg deliberately exits with an error after listing devices.
    let output = command(ffmpeg)
        .args([
            "-hide_banner",
            "-list_devices",
            "true",
            "-f",
            "dshow",
            "-i",
            "dummy",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Could not list microphones: {error}"))?;
    Ok(parse_audio_devices(&String::from_utf8_lossy(
        &output.stderr,
    )))
}

pub fn parse_audio_devices(log: &str) -> Vec<AudioDevice> {
    let mut devices: Vec<AudioDevice> = Vec::new();
    let mut audio_section = false;
    let mut last_audio: Option<usize> = None;
    for line in log.lines() {
        if line.contains("DirectShow audio devices") {
            audio_section = true;
        } else if line.contains("DirectShow video devices") {
            audio_section = false;
        }
        if line.contains("Alternative name") {
            if let (Some(index), Some(id)) = (last_audio.take(), quoted(line)) {
                devices[index].id = id.to_owned();
            }
        } else if line.contains("(audio)") || (audio_section && line.contains('"')) {
            if let Some(name) = quoted(line) {
                devices.push(AudioDevice {
                    name: name.into(),
                    id: name.into(),
                });
                last_audio = Some(devices.len() - 1);
            }
        } else {
            last_audio = None;
        }
    }
    devices
}

fn quoted(line: &str) -> Option<&str> {
    let start = line.find('"')? + 1;
    let end = line.rfind('"')?;
    (end > start).then_some(&line[start..end])
}

pub fn filename(text: &str) -> Result<String, String> {
    let name = text.trim();
    let stem = if name.to_ascii_lowercase().ends_with(".mp4") {
        &name[..name.len() - 4]
    } else {
        name
    };
    if stem.is_empty() {
        return Err("Enter a recording name in the top bar.".into());
    }
    if stem.ends_with([' ', '.'])
        || stem
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return Err("Use a filename without < > : \" / \\ | ? * or a trailing dot.".into());
    }
    let base = stem.split('.').next().unwrap_or(stem).to_uppercase();
    if matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|number| {
            matches!(
                number,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    }) {
        return Err("That name is reserved by Windows. Choose a different name.".into());
    }
    if stem.encode_utf16().count() > 180 {
        return Err("Keep the recording name under 180 characters.".into());
    }
    Ok(format!("{stem}.mp4"))
}

fn available_output_path(output: &Path) -> std::io::Result<PathBuf> {
    if !output.try_exists()? {
        return Ok(output.to_owned());
    }
    let stem = output.file_stem().unwrap_or_default();
    for number in 1u64.. {
        let mut name = stem.to_os_string();
        name.push(format!(" ({number}).mp4"));
        let candidate = output.with_file_name(name);
        if !candidate.try_exists()? {
            return Ok(candidate);
        }
    }
    unreachable!("exhausted all filename suffixes")
}

pub fn arguments(
    region: Region,
    codec: Codec,
    encoding: EncodingOptions,
    mic: Option<&str>,
    output: &Path,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "-hide_banner",
        "-loglevel",
        "warning",
        "-n",
        "-thread_queue_size",
        "512",
        "-f",
        "gdigrab",
        "-framerate",
        &encoding.fps.to_string(),
        "-draw_mouse",
        "1",
        "-offset_x",
        &region.x.to_string(),
        "-offset_y",
        &region.y.to_string(),
        "-video_size",
        &format!("{}x{}", region.width, region.height),
        "-i",
        "desktop",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    if let Some(device) = mic {
        args.extend(
            ["-thread_queue_size", "512", "-f", "dshow", "-i"]
                .into_iter()
                .map(OsString::from),
        );
        args.push(format!("audio={device}").into());
    }
    args.extend(["-map", "0:v:0", "-c:v"].into_iter().map(OsString::from));
    args.push(
        match codec {
            Codec::H264 => "libx264",
            Codec::H265 => "libx265",
        }
        .into(),
    );
    args.extend(
        [
            "-preset",
            "ultrafast",
            "-crf",
            &encoding.compression.to_string(),
            "-pix_fmt",
            "yuv420p",
        ]
        .into_iter()
        .map(OsString::from),
    );
    if codec == Codec::H265 {
        args.extend(
            ["-tag:v", "hvc1", "-x265-params", "log-level=error"]
                .into_iter()
                .map(OsString::from),
        );
    }
    if mic.is_some() {
        args.extend(
            [
                "-map",
                "1:a:0",
                "-c:a",
                "aac",
                "-b:a",
                "160k",
                "-af",
                "aresample=async=1:first_pts=0",
            ]
            .into_iter()
            .map(OsString::from),
        );
    } else {
        args.push("-an".into());
    }
    // Fragmented MP4 remains playable if the encoder or PC stops unexpectedly.
    args.extend(
        [
            "-movflags",
            "+frag_keyframe+empty_moov+default_base_moof",
            "-g",
            &(u32::from(encoding.fps) * 2).to_string(),
        ]
        .into_iter()
        .map(OsString::from),
    );
    args.push(output.as_os_str().into());
    args
}

pub struct Recording {
    child: Child,
    log: Arc<Mutex<Vec<u8>>>,
    reader: Option<thread::JoinHandle<()>>,
    pub output: PathBuf,
    pub started: Instant,
    pub stopping: bool,
    pub close_after: bool,
    stop_requested: Option<Instant>,
}

impl Recording {
    pub fn start(
        ffmpeg: &Path,
        region: Region,
        codec: Codec,
        encoding: EncodingOptions,
        mic: Option<&str>,
        output: PathBuf,
    ) -> Result<Self, String> {
        if region.width < 2 || region.height < 2 || region.width % 2 != 0 || region.height % 2 != 0
        {
            return Err("The recording area must have even dimensions of at least 2 × 2.".into());
        }
        encoding.validate()?;
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create the Videos folder: {error}"))?;
        }
        let output = available_output_path(&output)
            .map_err(|error| format!("Could not choose a recording filename: {error}"))?;
        let mut child = command(ffmpeg)
            .args(arguments(region, codec, encoding, mic, &output))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("Could not start FFmpeg: {error}"))?;
        let mut stderr = child.stderr.take().expect("piped stderr");
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = log.clone();
        let reader = thread::spawn(move || {
            let mut buffer = [0; 4096];
            while let Ok(count) = stderr.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                if let Ok(mut bytes) = sink.lock() {
                    bytes.extend_from_slice(&buffer[..count]);
                    if bytes.len() > 32_768 {
                        let excess = bytes.len() - 32_768;
                        bytes.drain(..excess);
                    }
                }
            }
        });
        Ok(Self {
            child,
            log,
            reader: Some(reader),
            output,
            started: Instant::now(),
            stopping: false,
            close_after: false,
            stop_requested: None,
        })
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if !self.stopping {
            self.stopping = true;
            self.stop_requested = Some(Instant::now());
            if let Some(mut stdin) = self.child.stdin.take() {
                stdin
                    .write_all(b"q\n")
                    .map_err(|error| format!("Could not signal FFmpeg to stop: {error}"))?;
            }
        }
        Ok(())
    }

    pub fn poll(&mut self) -> Option<Result<(), String>> {
        if self
            .stop_requested
            .is_some_and(|at| at.elapsed().as_secs() >= 15)
        {
            // A stuck audio driver must not leave the UI saving forever. Completed MP4 fragments survive.
            let _ = self.child.kill();
            self.stop_requested = None;
        }
        match self.child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => {
                if let Some(reader) = self.reader.take() {
                    let _ = reader.join();
                }
                let log = self
                    .log
                    .lock()
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                    .unwrap_or_default();
                let _ = fs::write(self.output.with_extension("ffmpeg.log"), &log);
                if status.success() && fs::metadata(&self.output).is_ok_and(|file| file.len() > 0) {
                    let _ = fs::remove_file(self.output.with_extension("ffmpeg.log"));
                    Some(Ok(()))
                } else {
                    let tail = log
                        .lines()
                        .rev()
                        .take(10)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join("\n");
                    Some(Err(format!(
                        "FFmpeg could not finish the recording.\n\n{tail}\n\nDetails: {}",
                        self.output.with_extension("ffmpeg.log").display()
                    )))
                }
            }
            Err(error) => Some(Err(format!("Could not check FFmpeg: {error}"))),
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        // Normal shutdown is asynchronous via stop()/poll(). This handles an unexpected UI teardown.
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.stop();
        }
    }
}

pub fn save_preferences(codec: Codec, mic: Option<&str>, encoding: EncodingOptions) {
    if let Some(path) = preferences_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut file) = File::create(path) {
            let _ = file.write_all(preferences_text(codec, mic, encoding).as_bytes());
        }
    }
}

fn preferences_text(codec: Codec, mic: Option<&str>, encoding: EncodingOptions) -> String {
    format!(
        "{}\n{}\n{}\n{}\n",
        codec.label(),
        mic.unwrap_or(""),
        encoding.fps,
        encoding.compression
    )
}

pub fn load_preferences() -> (Codec, Option<String>, EncodingOptions) {
    let content = preferences_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .unwrap_or_default();
    parse_preferences(&content)
}

fn parse_preferences(content: &str) -> (Codec, Option<String>, EncodingOptions) {
    let mut lines = content.lines();
    let codec = if lines.next() == Some("H.265") {
        Codec::H265
    } else {
        Codec::H264
    };
    let mic = lines
        .next()
        .filter(|line| !line.is_empty())
        .map(str::to_owned);
    let defaults = EncodingOptions::default();
    let fps = lines
        .next()
        .and_then(|line| line.parse().ok())
        .filter(|fps| (1..=240).contains(fps))
        .unwrap_or(defaults.fps);
    let compression = lines
        .next()
        .and_then(|line| line.parse().ok())
        .filter(|crf| *crf <= 51)
        .unwrap_or(defaults.compression);
    (codec, mic, EncodingOptions { fps, compression })
}

fn preferences_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("Frame Record/settings.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_existing_names_without_changing_originals() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("frame-record-{}-{unique}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let base = directory.join("Café 演示.mp4");
        let first = directory.join("Café 演示 (1).mp4");
        let second = directory.join("Café 演示 (2).mp4");
        let third = directory.join("Café 演示 (3).mp4");
        assert_eq!(available_output_path(&base).unwrap(), base);
        for path in [&base, &first, &third] {
            fs::write(path, b"existing video").unwrap();
        }
        assert_eq!(available_output_path(&base).unwrap(), second);
        fs::write(&second, b"another video").unwrap();
        assert_eq!(
            available_output_path(&base).unwrap(),
            directory.join("Café 演示 (4).mp4")
        );
        assert_eq!(fs::read(&base).unwrap(), b"existing video");
        for path in [&base, &first, &second, &third] {
            fs::remove_file(path).unwrap();
        }
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn preferences_preserve_settings_and_upgrade_old_files_to_60_fps() {
        let defaults = EncodingOptions {
            fps: 60,
            compression: 23,
        };
        assert_eq!(parse_preferences(""), (Codec::H264, None, defaults));
        assert_eq!(
            parse_preferences("H.265\nUSB mic\n"),
            (Codec::H265, Some("USB mic".into()), defaults)
        );
        let custom = EncodingOptions {
            fps: 144,
            compression: 31,
        };
        let content = preferences_text(Codec::H265, Some("Microphone 演示"), custom);
        assert_eq!(
            parse_preferences(&content),
            (Codec::H265, Some("Microphone 演示".into()), custom)
        );
        assert_eq!(
            parse_preferences("H.264\n\n24\n12\n"),
            (
                Codec::H264,
                None,
                EncodingOptions {
                    fps: 24,
                    compression: 12
                }
            )
        );
        assert_eq!(
            parse_preferences("H.264\n\n0\n255\n"),
            (Codec::H264, None, defaults)
        );
        assert_eq!(
            parse_preferences("H.264\n\n9999\ninvalid\n"),
            (Codec::H264, None, defaults)
        );
    }

    #[test]
    fn applies_frame_rate_compression_and_two_second_keyframe_interval_to_both_codecs() {
        let region = Region {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        };
        for codec in [Codec::H264, Codec::H265] {
            for encoding in [
                EncodingOptions::default(),
                EncodingOptions {
                    fps: 24,
                    compression: 32,
                },
                EncodingOptions {
                    fps: 240,
                    compression: 0,
                },
            ] {
                let args = arguments(region, codec, encoding, None, Path::new("test.mp4"));
                let option = |name: &str| {
                    args[args.iter().position(|arg| arg == name).unwrap() + 1].to_string_lossy()
                };
                assert_eq!(option("-framerate"), encoding.fps.to_string());
                assert_eq!(option("-crf"), encoding.compression.to_string());
                assert_eq!(option("-g"), (u32::from(encoding.fps) * 2).to_string());
            }
        }
        assert!(EncodingOptions {
            fps: 0,
            compression: 23
        }
        .validate()
        .is_err());
        assert!(EncodingOptions {
            fps: 241,
            compression: 23
        }
        .validate()
        .is_err());
        assert!(EncodingOptions {
            fps: 60,
            compression: 52
        }
        .validate()
        .is_err());
    }

    #[test]
    fn rejects_paths_and_reserved_windows_names() {
        for invalid in [
            "", " ", "../test", "a\\b", "C:video", "NUL", "con.txt", "LPT1", "COM²", "bad.", ".mp4",
        ] {
            assert!(filename(invalid).is_err(), "{invalid}");
        }
        assert_eq!(filename(" My recording.MP4 ").unwrap(), "My recording.mp4");
        assert_eq!(filename("Café 演示").unwrap(), "Café 演示.mp4");
    }

    #[test]
    fn parses_audio_only_and_preserves_unique_device_ids() {
        let devices = parse_audio_devices(
            r#"[dshow @ 1] "Camera" (video)
[dshow @ 1]   Alternative name "@video"
[dshow @ 1] "Microphone (USB)" (audio)
[dshow @ 1]   Alternative name "@device_cm_{first}"
[dshow @ 1] "Microphone (USB)" (audio)
[dshow @ 1]   Alternative name "@device_cm_{second}"
Error opening input file dummy."#,
        );
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "@device_cm_{first}");
        assert_eq!(devices[1].id, "@device_cm_{second}");
    }

    #[test]
    fn builds_both_codecs_and_separate_audio_input_without_shell_quoting() {
        let region = Region {
            x: -1920,
            y: 30,
            width: 1280,
            height: 720,
        };
        for codec in [Codec::H264, Codec::H265] {
            let args = arguments(
                region,
                codec,
                EncodingOptions::default(),
                Some("Microphone (USB)"),
                Path::new("a b.mp4"),
            );
            let args: Vec<_> = args
                .iter()
                .map(|s| s.to_string_lossy().to_string())
                .collect();
            assert!(args.contains(&"-1920".into()));
            assert!(args.contains(&"audio=Microphone (USB)".into()));
            assert!(args.contains(
                &if codec == Codec::H264 {
                    "libx264"
                } else {
                    "libx265"
                }
                .into()
            ));
            assert_eq!(args.last().unwrap(), "a b.mp4");
            assert!(args.contains(&"-n".into()));
            let silent = arguments(
                region,
                codec,
                EncodingOptions::default(),
                None,
                Path::new("out.mp4"),
            );
            assert!(silent.contains(&OsString::from("-an")));
            assert!(!silent.contains(&OsString::from("dshow")));
        }
    }
}
