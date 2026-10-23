//! Opt-in live integration check, excluded from release builds.
use super::*;
use std::{mem::zeroed, path::Path, ptr::null_mut};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};

pub struct Smoke {
    phase: u8,
    at: Instant,
    output: PathBuf,
    region: Option<recorder::Region>,
    shots: Vec<thread::JoinHandle<()>>,
    original: Vec<u8>,
    preview_only: bool,
    readme_only: bool,
    backdrop: Option<crate::screenshot::Backdrop>,
}

impl Smoke {
    pub fn new() -> Self {
        Self {
            phase: 0,
            at: Instant::now(),
            output: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("test-output")
                .join(native::default_name()),
            region: None,
            shots: Vec::new(),
            original: Vec::new(),
            preview_only: std::env::args()
                .any(|arg| arg == "--preview" || arg == "--readme-screenshot"),
            readme_only: std::env::args().any(|arg| arg == "--readme-screenshot"),
            backdrop: None,
        }
    }

    fn next(&mut self) {
        self.phase += 1;
        self.at = Instant::now();
    }

    pub fn tick(&mut self, app: &mut FrameRecord, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(50));
        assert!(app.error.is_none(), "{:?}", app.error);
        let elapsed = self.at.elapsed().as_secs_f32();
        match self.phase {
            0 if elapsed > 0.8 && app.devices_rx.is_none() => {
                std::fs::create_dir_all(&self.output).unwrap();
                let source_zip = self.output.join("frame-record-source.zip");
                source::save_to(&source_zip).unwrap();
                assert_eq!(std::fs::read(&source_zip).unwrap(), source::ARCHIVE);
                println!(
                    "Embedded source exported: {} ({} bytes)",
                    source_zip.display(),
                    source::ARCHIVE.len()
                );
                app.videos = self.output.clone();
                app.microphone = None;
                app.codec = Codec::H264;
                unsafe {
                    let hwnd = app.native.hwnd;
                    let mut bounds = zeroed();
                    GetWindowRect(hwnd, &mut bounds);
                    let scale = ctx.pixels_per_point();
                    assert_eq!(
                        hit(
                            hwnd,
                            bounds.left + (120.0 * scale) as i32,
                            bounds.top + (16.0 * scale) as i32
                        ),
                        HTCAPTION,
                        "Title strip must drag"
                    );
                    assert_eq!(
                        hit(hwnd, bounds.left + 1, (bounds.top + bounds.bottom) / 2),
                        HTLEFT
                    );
                    assert_eq!(
                        hit(hwnd, bounds.right - 2, bounds.bottom - 2),
                        HTBOTTOMRIGHT
                    );
                    let original = app.native.region();
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        bounds.left + 20,
                        bounds.top + 20,
                        bounds.right - bounds.left - 80,
                        bounds.bottom - bounds.top - 40,
                        SWP_NOZORDER,
                    );
                    app.native.update(scale, false, None);
                    let moved = app.native.region();
                    assert_eq!((moved.x - original.x, moved.y - original.y), (20, 20));
                    assert_eq!(
                        (moved.width - original.width, moved.height - original.height),
                        (-80, -40)
                    );
                    GetWindowRect(hwnd, &mut bounds);
                    let shape = CreateRectRgn(0, 0, 0, 0);
                    assert_ne!(GetWindowRgn(hwnd, shape), 0);
                    assert_eq!(
                        PtInRegion(shape, moved.x - bounds.left, moved.y - bounds.top),
                        0
                    );
                    assert_eq!(
                        PtInRegion(
                            shape,
                            moved.x - bounds.left + moved.width - 1,
                            moved.y - bounds.top + moved.height - 1
                        ),
                        0
                    );
                    assert_ne!(PtInRegion(shape, 0, 0), 0);
                    assert_ne!(
                        PtInRegion(
                            shape,
                            bounds.right - bounds.left - 1,
                            bounds.bottom - bounds.top - 1
                        ),
                        0
                    );
                    assert_ne!(
                        WindowFromPoint(POINT {
                            x: moved.x + 20,
                            y: moved.y + 20
                        }),
                        hwnd
                    );
                    DeleteObject(shape);
                    self.region = Some(moved);
                }
                println!(
                    "Modern egui frame: dragging, resizing, square cutout, click-through passed"
                );
                if self.readme_only {
                    app.name = "Recording".into();
                    app.encoding = EncodingOptions::default();
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(820.0, 460.0)));
                }
                self.next();
            }
            1 if elapsed > 0.7 => {
                if self.readme_only {
                    if self.backdrop.is_none() {
                        self.backdrop = Some(crate::screenshot::Backdrop::new(
                            app.native.hwnd,
                            ctx.pixels_per_point(),
                        ));
                        self.at = Instant::now();
                        return;
                    }
                    self.snapshot(app, "readme-frame.png");
                    self.phase = 13;
                    return;
                }
                self.snapshot(app, "modern-frame.png");
                self.next();
            }
            2 if elapsed > 0.7 => {
                app.settings = true;
                self.next();
            }
            3 if elapsed > 0.7 => {
                self.snapshot(app, "modern-settings.png");
                self.next();
            }
            4 if elapsed > 0.7 => {
                if self.preview_only {
                    self.phase = 13;
                    return;
                }
                app.settings = false;
                app.name = "Recording test".into();
                app.encoding = EncodingOptions {
                    fps: 60,
                    compression: 18,
                };
                app.start(ctx).unwrap();
                self.next();
            }
            5 if elapsed > 3.0 => {
                app.stop();
                self.next();
            }
            6 if app.recording.is_none() => {
                self.probe(app, "Recording test.mp4", "h264", 60, false);
                self.original = std::fs::read(self.output.join("Recording test.mp4")).unwrap();
                app.codec = Codec::H265;
                app.encoding = EncodingOptions {
                    fps: 24,
                    compression: 32,
                };
                app.start(ctx).unwrap();
                assert_eq!(
                    app.recording.as_ref().unwrap().output,
                    self.output.join("Recording test (1).mp4")
                );
                self.next();
            }
            7 if elapsed > 1.0 => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                self.next();
            }
            8 if elapsed > 0.4 => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                self.next();
            }
            9 if elapsed > 1.6 => {
                assert_eq!(app.native.region(), self.region.unwrap());
                app.stop();
                self.next();
            }
            10 if app.recording.is_none() => {
                self.probe(app, "Recording test (1).mp4", "hevc", 24, false);
                assert_eq!(
                    std::fs::read(self.output.join("Recording test.mp4")).unwrap(),
                    self.original
                );
                if let Some(device) = app.devices.first() {
                    app.microphone = Some(device.id.clone());
                    app.codec = Codec::H264;
                    app.encoding = EncodingOptions::default();
                    app.start(ctx).unwrap();
                    assert_eq!(
                        app.recording.as_ref().unwrap().output,
                        self.output.join("Recording test (2).mp4")
                    );
                    self.next();
                } else {
                    self.phase = 13;
                }
            }
            11 if elapsed > 3.0 => {
                app.close(ctx);
                self.next();
            }
            12 if app.recording.is_none() => {
                self.probe(app, "Recording test (2).mp4", "h264", 60, true);
                self.phase = 13;
            }
            13 => {
                for shot in self.shots.drain(..) {
                    shot.join().unwrap();
                }
                println!(
                    "{} passed: {}",
                    if self.preview_only {
                        "Visual preview and source export"
                    } else {
                        "Live checks"
                    },
                    self.output.display()
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                self.phase = 14;
            }
            _ => {}
        }
    }

    fn snapshot(&mut self, app: &FrameRecord, name: &str) {
        let mut rect: RECT = unsafe { zeroed() };
        unsafe {
            GetWindowRect(app.native.hwnd, &mut rect);
        }
        let ffmpeg = app.ffmpeg.clone().unwrap();
        let output = self.output.join(name);
        self.shots.push(thread::spawn(move || {
            let result = recorder::command(&ffmpeg)
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-f",
                    "gdigrab",
                    "-draw_mouse",
                    "0",
                    "-offset_x",
                    &rect.left.to_string(),
                    "-offset_y",
                    &rect.top.to_string(),
                    "-video_size",
                    &format!("{}x{}", rect.right - rect.left, rect.bottom - rect.top),
                    "-i",
                    "desktop",
                    "-frames:v",
                    "1",
                    "-update",
                    "1",
                ])
                .arg(&output)
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }));
    }

    fn probe(&self, app: &FrameRecord, name: &str, codec: &str, fps: u16, audio: bool) {
        let ffmpeg = app.ffmpeg.as_ref().unwrap();
        let path = self.output.join(name);
        let result = recorder::command(&ffmpeg.with_file_name("ffprobe.exe"))
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name,codec_type,width,height,r_frame_rate",
                "-of",
                "csv=p=0",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(result.status.success());
        let streams = String::from_utf8_lossy(&result.stdout);
        let r = self.region.unwrap();
        assert!(
            streams.contains(&format!("{codec},video,{},{},{fps}/1", r.width, r.height)),
            "{streams}"
        );
        assert_eq!(streams.contains("aac,audio"), audio);
        let result = recorder::command(ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(Path::new(&path))
            .args(["-f", "null", "-"])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        println!("Validated {name}: {}", streams.trim());
    }
}

unsafe fn hit(hwnd: HWND, x: i32, y: i32) -> u32 {
    SendMessageW(
        hwnd,
        WM_NCHITTEST,
        0,
        (x as u16 as u32 | ((y as u16 as u32) << 16)) as LPARAM,
    ) as u32
}
