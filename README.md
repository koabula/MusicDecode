# MusicDecode

**A small, offline NCM → MP3 desktop tool with a soft neumorphic interface.**

English | [简体中文](README.zh-CN.md)

[![Checks](https://github.com/koabula/MusicDecode/actions/workflows/ci.yml/badge.svg)](https://github.com/koabula/MusicDecode/actions/workflows/ci.yml)
[![Release](https://github.com/koabula/MusicDecode/actions/workflows/release.yml/badge.svg)](https://github.com/koabula/MusicDecode/actions/workflows/release.yml)
[![Download](https://img.shields.io/github/v/release/koabula/MusicDecode)](https://github.com/koabula/MusicDecode/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

Drag in your NCM files, choose an output folder, and start decoding. Everything happens locally. The application interface is currently in Simplified Chinese; this documentation is available in both languages.

![MusicDecode desktop interface](docs/images/musicdecode.png)

## Download

Get an installer from **[GitHub Releases](https://github.com/koabula/MusicDecode/releases/latest)**.

| Platform | Architecture | Download |
| --- | --- | --- |
| Windows 10 / 11 | x64 | `*-setup.exe` |
| macOS 12 or later | Apple Silicon / ARM64 | `*.dmg` |
| Linux, Ubuntu 22.04 baseline | x64 | `*.AppImage` or `*.deb` |

Intel Macs, Windows ARM64, and Linux ARM64 are not supported by this release. Node.js, Rust, .NET and a separate FFmpeg installation are not needed to run the packaged application.

### Installation

- **Windows:** run the setup executable. The installer supports English and Simplified Chinese and installs for the current user. WebView2 is required; the installer can install it when missing. Installers are not code-signed and Windows may display a reputation warning.
- **macOS:** open the DMG and drag MusicDecode to Applications. This release is ad-hoc signed, without Apple notarization. Gatekeeper may require approval in System Settings → Privacy & Security. See [Tauri's signing notes](https://v2.tauri.app/distribute/sign/macos/).
- **Linux AppImage:** make the downloaded file executable with `chmod +x MusicDecode*.AppImage`, then run it. AppImage compatibility depends on your distribution; FUSE or extraction mode may be needed.
- **Debian / Ubuntu:** install the downloaded DEB using `sudo apt install ./MusicDecode*.deb`. Package dependencies include GTK 3, WebKitGTK 4.1 and `xdg-utils`.

Verify downloads against the matching `SHA256SUMS-*.txt` release asset. SHA-256 checksums establish integrity; they are not a code-signing certificate.

## Features

- File selection, folder import, and native drag-and-drop.
- Batch queue with per-song progress, cancellation and retry.
- Original MP3 extraction without audio re-encoding.
- FLAC, M4A, WAV and Ogg audio transcoding to 192 / 256 / 320 kbps MP3.
- Embedded title, artist, album and JPEG/PNG artwork preservation.
- Source files stay untouched. Existing output files receive numbered alternatives.
- Complete audio decoding check before publishing each finished file.
- Responsive window layout: only the song list scrolls; the main actions remain visible.
- Offline operation, without accounts, remote artwork requests or telemetry.

## Usage

1. Click **添加文件** (Add files), **添加文件夹** (Add folder), or drop NCM files/folders into the window.
2. Optionally click **选择输出文件夹** (Choose output folder). By default, each source directory gets a `Decoded` subfolder.
3. Click **开始解码** (Start decoding), then **打开输出文件夹** (Open output folder) when complete.

Folder import includes subfolders only when its checkbox is enabled. Symbolic links are skipped. Each import supports up to 5,000 files. If outputs are spread across source folders, Open output folder opens the most recently completed location.

Cancellation removes the current incomplete temporary output; already completed files remain available. A failed song does not prevent the rest of the batch from being processed.

### Audio quality

| Actual audio inside NCM | Behavior |
| --- | --- |
| MP3 | Restore the original audio bytes and bitrate. The transcoding quality setting does not apply. |
| Other supported formats | Decrypt, then encode an MP3 at the selected bitrate. This is a lossy conversion. |

The decrypted audio header determines the actual format. The NCM metadata format field can be inaccurate. Renaming `.ncm` to `.mp3` does not decrypt it. Missing artwork is not downloaded, and raising the bitrate does not recover detail absent from the source.

## Development

Stack: **Tauri 2 + HTML/CSS/TypeScript + Rust**. FFmpeg is a separate, bundled audio executable.

Install Node.js 24, the stable Rust toolchain, and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS. Building the audio component also needs a C compiler, Make, `pkg-config` and a POSIX shell. Windows uses MSYS2 MINGW64 for the audio build and the MSVC toolchain for the Rust application; macOS uses Xcode Command Line Tools.

```sh
npm ci
npm run audio:sources
bash scripts/build-ffmpeg.sh
cargo fetch --locked --manifest-path src-tauri/Cargo.toml
node scripts/notices.mjs
npm run desktop
```

Run the audio build in the matching native build shell. `MUSICDECODE_TARGET` may be set explicitly to `x86_64-pc-windows-msvc`, `aarch64-apple-darwin` or `x86_64-unknown-linux-gnu`; otherwise it is detected from Rust. Cross-compiling the audio component is not supported by this script.

The source download script verifies SHA-256 hashes from [`third-party/sources.json`](third-party/sources.json). The build uses FFmpeg 9.0.2 and LAME 3.100, disables networking, GPL and nonfree components, and includes only the required audio functionality. Embedded JPEG/PNG artwork is preserved by the Rust metadata layer.

### Checks and packages

```sh
npm run build
npm run check:version
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --locked --manifest-path src-tauri/Cargo.toml
npm run package
```

Prepare the audio sidecar before testing. Synthetic tests generate their own audio and exercise MP3 preservation, FLAC conversion, filenames, cancellation, corruption and output collision handling. An additional optional real-file regression uses `MUSICDECODE_TEST_NCM` and `MUSICDECODE_TEST_FFMPEG`; personal songs are never required by CI.

Native installers are written to `src-tauri/target/release/bundle`, or `src-tauri/target/<target>/release/bundle` when a target is explicit. Windows developers can also run `npm run package:portable` to stage a portable directory in `release`.

## Automated releases

[`release.yml`](.github/workflows/release.yml) builds Windows x64, macOS ARM64 and Linux x64 on native GitHub runners. Each job builds its audio component from pinned sources, runs conversion tests, builds installers and uploads checksums. The macOS job also verifies the app's ad-hoc signature.

The publish job runs only after every platform succeeds. It checks all artifact hashes, assembles the release as a draft, uploads every asset, and then publishes it. Complete FFmpeg/LAME source archives and the build recipe accompany the installers.

To release a version, update `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json` together; add `docs/releases/vX.Y.Z.md`; then push the matching `vX.Y.Z` tag or run the Release workflow manually. Already published releases are not overwritten. Publishing uses the workflow's `GITHUB_TOKEN`; no personal access token is required.

## Troubleshooting

- **File rejected:** the input may be truncated, not actually an NCM file, or use an unsupported encryption variant. Keep the original and inspect the error shown in the song list.
- **Audio component missing:** reinstall the complete application. For portable copies, keep `musicdecode-ffmpeg.exe` beside `MusicDecode.exe`.
- **Output folder cannot be written:** choose a writable folder and retry.
- **No artwork:** only existing JPEG/PNG artwork in the NCM is embedded.
- **Linux does not start:** check the distribution's WebKitGTK/GTK dependencies and AppImage requirements. A successful build is not a guarantee for every Linux distribution.

## License and acknowledgements

MusicDecode's application code is [MIT licensed](LICENSE). The independent FFmpeg/LAME audio component uses its own LGPL licenses. Installed distributions include audio licenses and build details under `licenses/audio`; dependency notices are in [`THIRD-PARTY-NOTICES.txt`](THIRD-PARTY-NOTICES.txt). Matching audio source archives and the build recipe are available with each release.

Built with [Tauri](https://tauri.app/), [RustCrypto](https://github.com/RustCrypto), [rust-id3](https://github.com/polyfloyd/rust-id3), [FFmpeg](https://ffmpeg.org/) and [LAME](https://lame.sourceforge.io/). Thanks to the [ncmdump community](https://github.com/taurusxin/ncmdump) for documenting the NCM format.

Bug reports and focused pull requests are welcome. Include your OS, application version, steps to reproduce and the displayed error; do not attach copyrighted songs or personal files to public issues.
