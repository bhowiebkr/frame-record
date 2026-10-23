# Frame Record

A simple screen recorder for Windows, written in Rust.

Move and resize the frame around what you want to record. Click **Record**. Click **Stop** when you're done.

![Frame Record with its transparent center shown over a checkerboard background](assets/frame-record.png)

## Use

1. Open `frame-record.exe`.
2. Drag the title bar to move the frame. Drag an edge or corner to resize it.
3. Enter a filename in the top bar.
4. Click **Record**, then **Stop**.

Recordings are saved as MP4 files in your Windows **Videos** folder. Existing names get a number: `Recording.mp4`, `Recording (1).mp4`, `Recording (2).mp4`.

The center is fully transparent and lets you use the app underneath. Only the area inside the border is recorded. The toolbar and border stay out of the video. The frame stays fixed while recording.

## Settings

- **Codec:** H.264 or H.265.
- **Frame rate:** defaults to 60 FPS.
- **Compression:** lower for better quality, higher for smaller files.
- **Microphone:** optional; off by default.

Your settings are remembered.

## Run or build

Requires 64-bit Windows 10 or 11. Keep `ffmpeg.exe` beside `frame-record.exe`.

To run it without building, download `frame-record-v<version>-windows-x64.zip` from [Releases](https://github.com/bhowiebkr/frame-record/releases), extract it, and open `frame-record.exe`. FFmpeg is included. The app is not code-signed, so Windows SmartScreen may ask you to confirm the first launch.

To build from source, install Rust with the MSVC toolchain and Visual Studio C++ Build Tools, then run:

```powershell
.\scripts\build.ps1 -WithFFmpeg
```

This downloads portable FFmpeg and puts the ready-to-run tool in `dist/Frame Record`. Add `-Package` to also create the release ZIP in `dist`.

## Get the source

Open **Settings → Save source code…** to export the small source ZIP embedded in the tool. It contains the matching source, assets, and build scripts for that version.

## License

[MIT](LICENSE). FFmpeg has its own license; see [third-party notices](THIRD-PARTY.md).
