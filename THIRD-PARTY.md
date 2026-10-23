# FFmpeg

The optional setup script downloads FFmpeg 9.0.2 essentials from Gyan, a Windows build provider linked from https://ffmpeg.org/download.html.

Build archive: https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2-essentials_build.zip

Archive SHA-256: `60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba`

Build information and corresponding source links: https://www.gyan.dev/ffmpeg/builds/

FFmpeg project and source: https://ffmpeg.org/ and https://git.ffmpeg.org/ffmpeg.git

This FFmpeg build includes GPL components (including x264 and x265) and is separately licensed under GPL version 3 or later. Its license is included as `FFmpeg-LICENSE.txt` in the portable output. Frame Record invokes the executable as a separate process. Review the build provider's corresponding source and redistribution requirements before redistributing FFmpeg binaries.

`windows-sys` and `windows-link` are from Microsoft's Rust for Windows project, distributed under MIT OR Apache-2.0: https://github.com/microsoft/windows-rs

The native UI uses egui / eframe (MIT OR Apache-2.0): https://github.com/emilk/egui. Its window and graphics stack includes winit (Apache-2.0), glutin (MIT OR Apache-2.0), and glow (MIT OR Apache-2.0). Exact versions and transitive dependencies are recorded in `Cargo.lock`. The application uses the OpenGL renderer and does not bundle a browser runtime.

Native save dialogs use rfd (MIT): https://github.com/PolyMeilex/rfd. Build-time icon embedding uses winresource (MIT): https://github.com/BenjaminRi/winresource. Build-time source compression uses zip (MIT): https://github.com/zip-rs/zip2. Neither the resource compiler nor the ZIP writer is required at runtime.
