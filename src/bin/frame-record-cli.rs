//! Command-line recorder for scripts and AI agents. Success prints at most one line to stdout;
//! failures print `error: ...` to stderr and exit 1 (2 for usage errors).
use frame_record::{
    native,
    recorder::{self, Codec, EncodingOptions, Recording, Region},
};
use std::{
    fs,
    io::Write,
    mem::{size_of, zeroed},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    ptr::{null, null_mut},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use windows_sys::{
    core::BOOL,
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::{Console::*, Threading::*},
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};

const HELP: &str = "\
frame-record-cli: record part of the screen to MP4. Coordinates are physical pixels.
  record [TARGET] [OPTIONS]  record until -t elapses, Ctrl+C, or `stop`; prints the file path
  start [TARGET] [OPTIONS]   record in the background; prints the file path once capturing
  stop                       finish the running recording; prints the file path
  windows                    list visible windows: X,Y,W,H title
  screens                    list screens: N X,Y,W,H [primary]
  mics                       list microphone names
TARGET (default: --screen 0):
  --region X,Y,W,H  --window TITLE (case-insensitive substring)  --screen N
OPTIONS:
  -t, --seconds S   stop after S seconds
  -o, --output P    file or folder (default: Videos\\Recording <date time>.mp4)
  --fps N (60)  --crf N (23; 0-51, lower is better)  --codec h264|h265  --mic [NAME]
Example: frame-record-cli record --window Notepad -t 5 -o out\\";

const STOP_EVENT: &str = "Local\\FrameRecord.Stop";
const VALUE_FLAGS: [&str; 9] = [
    "--region",
    "--window",
    "--screen",
    "-t",
    "--seconds",
    "-o",
    "--output",
    "--fps",
    "--crf",
];

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

enum Failure {
    Usage(String),
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
enum Target {
    Screen(usize),
    Region(Region),
    Window(String),
}

#[derive(Debug, PartialEq)]
struct Options {
    target: Target,
    seconds: Option<f64>,
    output: Option<String>,
    codec: Codec,
    encoding: EncodingOptions,
    /// `Some("")` selects the first microphone.
    mic: Option<String>,
}

fn main() -> ExitCode {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (command, rest) = match args.split_first() {
        Some((command, rest)) => (command.as_str(), rest),
        None => ("help", &[][..]),
    };
    let result = match command {
        "record" | "start" => parse(rest).map_err(Failure::Usage).and_then(|options| {
            if command == "record" {
                record(&options)
            } else {
                start(rest)
            }
            .map_err(Failure::Error)
        }),
        "stop" | "windows" | "screens" | "mics" if !rest.is_empty() => {
            Err(Failure::Usage(format!("`{command}` takes no options")))
        }
        "stop" => stop().map_err(Failure::Error),
        "windows" => Ok(windows()
            .iter()
            .map(|window| format!("{} {}", region_text(window.1), window.2))
            .collect::<Vec<_>>()
            .join("\n")),
        "screens" => Ok(screens()
            .iter()
            .enumerate()
            .map(|(index, (region, primary))| {
                let flag = if *primary { " primary" } else { "" };
                format!("{index} {}{flag}", region_text(*region))
            })
            .collect::<Vec<_>>()
            .join("\n")),
        "mics" => ffmpeg()
            .and_then(|ffmpeg| recorder::list_audio_devices(&ffmpeg))
            .map(|devices| {
                devices
                    .into_iter()
                    .map(|device| device.name)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .map_err(Failure::Error),
        "help" | "-h" | "--help" => Ok(HELP.into()),
        _ => Err(Failure::Usage(format!("unknown command `{command}`"))),
    };
    // writeln! rather than println!, which panics when the reader has gone away.
    match result {
        Ok(text) => {
            if !text.is_empty() {
                let _ = writeln!(std::io::stdout(), "{text}");
            }
            ExitCode::SUCCESS
        }
        Err(Failure::Usage(message)) => {
            let _ = writeln!(std::io::stderr(), "error: {message}\n{HELP}");
            ExitCode::from(2)
        }
        Err(Failure::Error(message)) => {
            let _ = writeln!(std::io::stderr(), "error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        target: Target::Screen(0),
        seconds: None,
        output: None,
        codec: Codec::H264,
        encoding: EncodingOptions::default(),
        mic: None,
    };
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        index += 1;
        if flag == "--mic" {
            let name = args.get(index).filter(|name| !name.starts_with('-'));
            index += name.is_some() as usize;
            options.mic = Some(name.cloned().unwrap_or_default());
            continue;
        }
        if flag == "--codec" {
            let value = args.get(index).ok_or("--codec needs a value")?;
            index += 1;
            options.codec = match value.to_ascii_lowercase().as_str() {
                "h264" | "h.264" | "avc" => Codec::H264,
                "h265" | "h.265" | "hevc" => Codec::H265,
                _ => return Err(format!("--codec must be h264 or h265, not `{value}`")),
            };
            continue;
        }
        if !VALUE_FLAGS.contains(&flag) {
            return Err(format!("unknown option `{flag}`"));
        }
        let value = args
            .get(index)
            .ok_or_else(|| format!("{flag} needs a value"))?;
        index += 1;
        match flag {
            "--region" => {
                // PowerShell passes an unquoted 1,2,3,4 as four separate arguments.
                let mut text = value.clone();
                while pieces(&text).count() < 4 {
                    match args.get(index) {
                        Some(next) if pieces(next).all(|piece| piece.parse::<i32>().is_ok()) => {
                            text = format!("{text},{next}");
                            index += 1;
                        }
                        _ => break,
                    }
                }
                options.target = Target::Region(parse_region(&text)?);
            }
            "--window" => options.target = Target::Window(value.clone()),
            "--screen" => options.target = Target::Screen(number(flag, value)?),
            "-t" | "--seconds" => {
                let seconds: f64 = number(flag, value)?;
                if !(seconds > 0.0 && seconds.is_finite()) {
                    return Err(format!("{flag} must be a positive number of seconds"));
                }
                options.seconds = Some(seconds);
            }
            "-o" | "--output" => options.output = Some(value.clone()),
            "--fps" => options.encoding.fps = number(flag, value)?,
            "--crf" => options.encoding.compression = number(flag, value)?,
            _ => unreachable!(),
        }
    }
    Ok(options)
}

fn number<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, String> {
    value
        .trim()
        .parse()
        .map_err(|_| format!("{flag} expects a number, not `{value}`"))
}

fn pieces(text: &str) -> impl Iterator<Item = &str> {
    text.split([',', ' ']).filter(|piece| !piece.is_empty())
}

fn parse_region(text: &str) -> Result<Region, String> {
    let parts: Vec<i32> = pieces(text)
        .map(|part| number("--region", part))
        .collect::<Result<_, _>>()?;
    match parts[..] {
        [x, y, width, height] if width > 0 && height > 0 => Ok(Region {
            x,
            y,
            width,
            height,
        }),
        _ => Err(format!(
            "--region expects X,Y,W,H with positive W and H, not `{text}`"
        )),
    }
}

fn region_text(region: Region) -> String {
    format!(
        "{},{},{},{}",
        region.x, region.y, region.width, region.height
    )
}

/// Clips to the desktop and rounds the size down to even numbers, which the encoders require.
fn fit(region: Region, desktop: Region) -> Result<Region, String> {
    let left = region.x.max(desktop.x);
    let top = region.y.max(desktop.y);
    let width = ((region.x + region.width).min(desktop.x + desktop.width) - left) & !1;
    let height = ((region.y + region.height).min(desktop.y + desktop.height) - top) & !1;
    if width < 2 || height < 2 {
        return Err(format!(
            "{} is off screen; the desktop is {}",
            region_text(region),
            region_text(desktop)
        ));
    }
    Ok(Region {
        x: left,
        y: top,
        width,
        height,
    })
}

fn desktop() -> Region {
    unsafe {
        Region {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

fn resolve(target: &Target) -> Result<Region, String> {
    let region = match target {
        Target::Region(region) => *region,
        Target::Screen(index) => screens()
            .get(*index)
            .map(|screen| screen.0)
            .ok_or_else(|| format!("no screen {index}; run `frame-record-cli screens`"))?,
        Target::Window(title) => {
            let wanted = title.to_lowercase();
            let (hwnd, ..) = windows()
                .into_iter()
                .find(|window| window.2.to_lowercase().contains(&wanted))
                .ok_or_else(|| {
                    format!("no visible window title contains `{title}`; run `frame-record-cli windows`")
                })?;
            // The capture is a fixed screen area, so the window should be on top. Windows may refuse.
            unsafe { SetForegroundWindow(hwnd) };
            thread::sleep(Duration::from_millis(200));
            unsafe { window_region(hwnd) }.ok_or("the window closed")?
        }
    };
    fit(region, desktop())
}

fn windows() -> Vec<(HWND, Region, String)> {
    unsafe extern "system" fn each(hwnd: HWND, list: LPARAM) -> BOOL {
        let list = &mut *(list as *mut Vec<(HWND, Region, String)>);
        let tool = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0;
        let mut cloaked = 0u32;
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            (&mut cloaked as *mut u32).cast(),
            size_of::<u32>() as u32,
        );
        let length = GetWindowTextLengthW(hwnd);
        if IsWindowVisible(hwnd) == 0
            || IsIconic(hwnd) != 0
            || tool
            || cloaked != 0
            || length == 0
            || hwnd == GetShellWindow()
        {
            return TRUE;
        }
        let mut text = vec![0u16; length as usize + 1];
        let copied = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32).max(0) as usize;
        let title = String::from_utf16_lossy(&text[..copied]).replace(['\r', '\n', '\t'], " ");
        if let Some(region) = window_region(hwnd).filter(|r| r.width > 0 && r.height > 0) {
            list.push((hwnd, region, title));
        }
        TRUE
    }
    let mut list = Vec::new();
    unsafe { EnumWindows(Some(each), &mut list as *mut _ as LPARAM) };
    list
}

/// The visible frame, without the invisible resize borders that GetWindowRect includes.
unsafe fn window_region(hwnd: HWND) -> Option<Region> {
    let mut rect: RECT = zeroed();
    let bounds = DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS as u32,
        (&mut rect as *mut RECT).cast(),
        size_of::<RECT>() as u32,
    );
    if bounds < 0 && GetWindowRect(hwnd, &mut rect) == 0 {
        return None;
    }
    Some(Region {
        x: rect.left,
        y: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    })
}

/// Screens with the primary one first.
fn screens() -> Vec<(Region, bool)> {
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, list: LPARAM) -> BOOL {
        let list = &mut *(list as *mut Vec<(Region, bool)>);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) != 0 {
            let rect = info.rcMonitor;
            list.push((
                Region {
                    x: rect.left,
                    y: rect.top,
                    width: rect.right - rect.left,
                    height: rect.bottom - rect.top,
                },
                info.dwFlags & MONITORINFOF_PRIMARY != 0,
            ));
        }
        TRUE
    }
    let mut list: Vec<(Region, bool)> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(each),
            &mut list as *mut _ as LPARAM,
        )
    };
    list.sort_by_key(|screen| !screen.1);
    list
}

fn ffmpeg() -> Result<PathBuf, String> {
    recorder::find_ffmpeg().ok_or_else(|| {
        "FFmpeg not found; put ffmpeg.exe beside frame-record-cli.exe or set FRAME_RECORD_FFMPEG"
            .into()
    })
}

fn output_path(output: Option<&str>) -> Result<PathBuf, String> {
    let name = format!("{}.mp4", native::default_name());
    let path = match output {
        None => native::videos_folder()?.join(name),
        Some(text) if text.ends_with(['\\', '/']) || Path::new(text).is_dir() => {
            Path::new(text).join(name)
        }
        Some(text) if Path::new(text).extension().is_none() => {
            Path::new(text).with_extension("mp4")
        }
        Some(text) => PathBuf::from(text),
    };
    std::path::absolute(&path).map_err(|error| format!("bad output path: {error}"))
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn recorder_running() -> bool {
    let name = wide(STOP_EVENT);
    let event = unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) };
    if event.is_null() {
        return false;
    }
    drop(Handle(event));
    true
}

/// `stop` and `start` learn what the recorder is doing from this file: `pid<TAB>state<TAB>text`.
fn state_file() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Frame Record")
        .join("cli.txt")
}

fn write_state(state: &str, text: &str) {
    let path = state_file();
    let temporary = path.with_extension("tmp");
    let line = format!(
        "{}\t{state}\t{}\n",
        std::process::id(),
        text.replace('\n', " ")
    );
    if fs::create_dir_all(path.parent().unwrap()).is_ok() && fs::write(&temporary, line).is_ok() {
        let _ = fs::rename(&temporary, &path);
    }
}

fn read_state() -> Option<(u32, String, String)> {
    let text = fs::read_to_string(state_file()).ok()?;
    let mut parts = text.trim_end().splitn(3, '\t');
    let pid = parts.next()?.parse().ok()?;
    Some((pid, parts.next()?.into(), parts.next().unwrap_or("").into()))
}

unsafe extern "system" fn interrupted(_: u32) -> BOOL {
    INTERRUPTED.store(true, Ordering::Relaxed);
    TRUE
}

fn record(options: &Options) -> Result<String, String> {
    let name = wide(STOP_EVENT);
    let event = unsafe { CreateEventW(null(), 1, 0, name.as_ptr()) };
    if event.is_null() {
        return Err("could not create the stop signal".into());
    }
    let event = Handle(event);
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Err("another recording is running; finish it with `frame-record-cli stop`".into());
    }
    write_state("starting", "");
    unsafe { SetConsoleCtrlHandler(Some(interrupted), 1) };
    let result = capture(options, event.0);
    match &result {
        Ok(path) => write_state("saved", path),
        Err(error) => write_state("error", error),
    }
    result
}

fn capture(options: &Options, stop: HANDLE) -> Result<String, String> {
    let ffmpeg = ffmpeg()?;
    let region = resolve(&options.target)?;
    let mic = match &options.mic {
        None => None,
        Some(wanted) => {
            let lower = wanted.to_lowercase();
            let device = recorder::list_audio_devices(&ffmpeg)?
                .into_iter()
                .find(|device| device.name.to_lowercase().contains(&lower))
                .ok_or_else(|| {
                    format!("no microphone matches `{wanted}`; run `frame-record-cli mics`")
                })?;
            Some(device.id)
        }
    };
    let output = output_path(options.output.as_deref())?;
    let mut recording = Recording::start(
        &ffmpeg,
        region,
        options.codec,
        options.encoding,
        mic.as_deref(),
        output,
    )?;
    let early_exit =
        |result: Result<(), String>| result.and(Err("FFmpeg exited early".to_string()));
    // FFmpeg writes the MP4 header once the screen and microphone are open.
    let opening = Instant::now();
    while !fs::metadata(&recording.output).is_ok_and(|file| file.len() > 0) {
        if let Some(result) = recording.poll() {
            return early_exit(result);
        }
        if opening.elapsed() > Duration::from_secs(15) {
            return Err("FFmpeg did not start capturing within 15 seconds".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    let path = recording.output.display().to_string();
    write_state("recording", &path);
    let started = Instant::now();
    while unsafe { WaitForSingleObject(stop, 50) } != WAIT_OBJECT_0
        && !INTERRUPTED.load(Ordering::Relaxed)
        && options
            .seconds
            .is_none_or(|seconds| started.elapsed().as_secs_f64() < seconds)
    {
        if let Some(result) = recording.poll() {
            return early_exit(result);
        }
    }
    recording.stop()?;
    loop {
        if let Some(result) = recording.poll() {
            return result.map(|_| path);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn start(args: &[String]) -> Result<String, String> {
    if recorder_running() {
        return Err("another recording is running; finish it with `frame-record-cli stop`".into());
    }
    // An inherited copy of our stdout would make the calling shell wait for the background recorder.
    unsafe {
        for handle in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            SetHandleInformation(GetStdHandle(handle), HANDLE_FLAG_INHERIT, 0);
        }
    }
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let spawn = |flags| {
        Command::new(&exe)
            .arg("record")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(flags)
            .spawn()
    };
    let detached = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    // Breaking away keeps the recorder alive when the caller's job ends; some jobs forbid it.
    let mut child = spawn(detached | CREATE_BREAKAWAY_FROM_JOB)
        .or_else(|_| spawn(detached))
        .map_err(|error| format!("could not start the recorder: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let exited = child.try_wait().ok().flatten().is_some();
        if let Some((_, state, text)) = read_state().filter(|state| state.0 == child.id()) {
            match state.as_str() {
                "recording" | "saved" => return Ok(text),
                "error" => return Err(text),
                _ => {}
            }
        }
        if exited {
            return Err("the recorder exited unexpectedly".into());
        }
        if Instant::now() > deadline {
            return Err("timed out waiting for the recorder to start".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn stop() -> Result<String, String> {
    let name = wide(STOP_EVENT);
    let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
    if event.is_null() {
        return Err("no recording is running".into());
    }
    let owner = read_state().map(|state| state.0);
    unsafe { SetEvent(event) };
    drop(Handle(event));
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let running = recorder_running();
        if let Some((pid, state, text)) = read_state() {
            if owner.is_none_or(|owner| owner == pid) {
                match state.as_str() {
                    "saved" => return Ok(text),
                    "error" => return Err(text),
                    _ => {}
                }
            }
        }
        if !running {
            return Err("the recorder exited without saving".into());
        }
        if Instant::now() > deadline {
            return Err("timed out waiting for the recording to finish".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_targets_and_options() {
        let options = parse(&args(
            "--region 10,-20,640,480 -t 2.5 -o out\\ --fps 30 --crf 18 --codec hevc --mic",
        ))
        .unwrap();
        assert_eq!(
            options.target,
            Target::Region(Region {
                x: 10,
                y: -20,
                width: 640,
                height: 480
            })
        );
        assert_eq!(options.seconds, Some(2.5));
        assert_eq!(options.output.as_deref(), Some("out\\"));
        assert_eq!(
            options.encoding,
            EncodingOptions {
                fps: 30,
                compression: 18
            }
        );
        assert_eq!(options.codec, Codec::H265);
        assert_eq!(options.mic.as_deref(), Some(""));
        let options = parse(&args("--mic Yeti --window notepad")).unwrap();
        assert_eq!(options.mic.as_deref(), Some("Yeti"));
        assert_eq!(options.target, Target::Window("notepad".into()));
        let split = parse(&args("--region 10, -20, 640 480 -t 1")).unwrap();
        assert_eq!(
            split.target,
            parse(&args("--region 10,-20,640,480")).unwrap().target
        );
        assert_eq!(split.seconds, Some(1.0));
        assert_eq!(parse(&[]).unwrap().target, Target::Screen(0));
    }

    #[test]
    fn rejects_bad_arguments() {
        for bad in [
            "--region 1,2,3",
            "--region 0,0,0,10",
            "--screen x",
            "-t 0",
            "-t",
            "--codec vp9",
            "--loud",
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn fits_regions_to_the_desktop() {
        let desktop = Region {
            x: -1920,
            y: 0,
            width: 4480,
            height: 1440,
        };
        let inside = Region {
            x: 100,
            y: 50,
            width: 641,
            height: 481,
        };
        assert_eq!(
            fit(inside, desktop),
            Ok(Region {
                width: 640,
                height: 480,
                ..inside
            })
        );
        let overhanging = Region {
            x: -2000,
            y: -10,
            width: 300,
            height: 200,
        };
        assert_eq!(
            fit(overhanging, desktop),
            Ok(Region {
                x: -1920,
                y: 0,
                width: 220,
                height: 190
            })
        );
        assert!(fit(Region { x: 5000, ..inside }, desktop).is_err());
    }
}
