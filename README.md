<div align="center">

# ⬇️ Todo Downloader

**A lightweight, bloat-free desktop download manager.**
Written in Rust. Single portable executable — no installer, no runtime, no Java.

[![Build](https://github.com/AcidClawX41/todo-downloader/actions/workflows/build.yml/badge.svg)](https://github.com/AcidClawX41/todo-downloader/actions/workflows/build.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)
![egui](https://img.shields.io/badge/GUI-egui%200.28-blue)
![Platforms](https://img.shields.io/badge/Windows%20%7C%20Linux%20%7C%20macOS-lightgrey)

</div>

---

## What is this?

A download manager in the spirit of JDownloader2 — but **without Java, without an installer, without adware and without telemetry**. A single ~7 MB executable that starts instantly.

It started as a tool to grab full TikTok and Douyin profiles in maximum quality, and grew into something that handles 1000+ sites.

## Features

### Downloading

- **Universal**: direct file links over native HTTP, and pages from 1000+ sites (YouTube, TikTok, Instagram, X, Reddit, Twitch, Weibo, Bilibili…) through the built-in engines.
- **Always maximum quality.** For ByteDance images (TikTok/Douyin) it detects the CDN's `~tplv-` processing template — which applies watermarks and downscaling — and requests the unprocessed original first. The difference is real: from a recompressed thumbnail to **2160×2880, watermark-free**.
- **Adaptive video quality**: with ffmpeg installed it merges separate video and audio streams (1080p+ on YouTube); without it, it falls back to the best pre-merged file instead of failing.
- Queue with **concurrent downloads** (1–8), per-file and global progress and speed.
- **Real pause and resume** using HTTP Range over `.part` files.
- Exponential backoff retries and request spacing to avoid triggering rate limits.

### Capturing links

- **LinkGrabber**: watches the clipboard and queues URLs as you copy them, just like JDownloader.
- **Profile view**: paste a TikTok profile URL, analyze it, then pick with checkboxes which videos and/or image posts you want. For Instagram, Weibo or Pinterest it downloads the whole profile.
- **Browser capture (Click'n'Load)**: a local HTTP receiver accepts links captured by a script running inside the profile tab itself. This solves what no external tool can: Douyin profiles and session-gated content, because the script inherits your cookies and the site's API signatures.
- TXT/JSON import and drag-and-drop onto the window.

### Built-in engines

All three install **with one click from Settings**, downloading the official binary from GitHub Releases. No Python, no pip, no PATH setup.

| Engine | Purpose | Source |
|---|---|---|
| **yt-dlp** | Video pages (1000+ sites) | `github.com/yt-dlp/yt-dlp` |
| **gallery-dl** | Galleries, carousels and image profiles | `github.com/gdl-org/builds` |
| **ffmpeg** | Merging video+audio for maximum quality | `github.com/yt-dlp/FFmpeg-Builds` |

Each binary is verified by running it after download; if it doesn't respond, it's deleted.

### Interface

- Dark theme, sidebar navigation, stat cards and an animated *gloss* hover effect.
- **English and Spanish**, with automatic system-language detection and hot switching.
- CJK font support: Chinese, Japanese and Korean titles render correctly.
- Browser cookies (Firefox recommended) or a `cookies.txt` file.

## Installation

### Prebuilt binaries (recommended)

Download the one for your system from [**Releases**](../../releases):

| System | File |
|---|---|
| Windows 10/11 | `todo-downloader-windows-x86_64.exe` |
| Linux | `todo-downloader-linux-x86_64` |
| macOS Apple Silicon | `todo-downloader-macos-aarch64` |
| macOS Intel | `todo-downloader-macos-x86_64` |

On Linux and macOS, make it executable: `chmod +x todo-downloader-*`

Verify your download with the `.sha256` file that accompanies each binary:

```bash
sha256sum -c todo-downloader-linux-x86_64.sha256
```

### Building from source

You need [Rust](https://rustup.rs). On Linux, also:

```bash
sudo apt install libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
                 libxkbcommon-dev libxkbcommon-x11-dev \
                 libwayland-dev libgl1-mesa-dev libegl1-mesa-dev pkg-config
```

> GTK is not required — file dialogs use the XDG Desktop Portal.

```bash
git clone https://github.com/AcidClawX41/todo-downloader
cd todo-downloader
cargo build --release
```

The binary lands in `target/release/`. On Windows you can also double-click `Compilar.bat`.

## Getting started

1. Open the app, go to **Settings** and install **yt-dlp**, **gallery-dl** and **ffmpeg** (one click each).
2. Pick your download folder.
3. Then use whichever path fits:

| I want to… | How |
|---|---|
| Grab a single video | Copy the URL — the LinkGrabber picks it up automatically |
| Grab a TikTok profile | **Profile** tab → analyze → select → download |
| Grab an Instagram/Weibo profile | **Profile** tab → downloaded whole via gallery-dl |
| Grab a Douyin profile | **Capture** tab → copy script → paste into the browser console (F12) |
| Queue a list of links | **Add links** or **Import TXT/JSON** |

### About cookies

Some sites (Douyin, Instagram, Weibo, age-restricted content) require a signed-in session.

> ⚠️ **Chrome 127+, Edge, Brave and Opera encrypt cookies with App-Bound Encryption**: no external tool can read them, not even with the browser closed. This is not a bug in this application.

Two alternatives that do work:

1. **Firefox** — not affected. Sign in there and select it in Settings.
2. **A `cookies.txt` file** (most reliable) — export it with an extension like *Get cookies.txt LOCALLY* and select it in Settings.

## Known limitations

Being upfront about these:

- **Douyin profiles cannot be enumerated** by yt-dlp or gallery-dl — no profile extractor exists for that site. That is precisely why the **Capture** tab exists: it solves the case from inside the browser.
- Direct CDN links **expire within hours**. If you capture thousands of files, the earliest ones may expire before their turn comes. Download in batches.
- Pausing a yt-dlp or gallery-dl task doesn't kill the subprocess; it finishes the file in progress.
- The binary is **not code-signed**, so SmartScreen or your antivirus may warn about it. It's a reputation-based false positive — the hash and the full source are published here.
- Automatic ffmpeg installation is Windows-only; on Linux and macOS use your package manager.

## Architecture

```
src/
├── main.rs       UI (egui) + download engine (tokio/reqwest)
├── i18n.rs       EN/ES translations — adding languages is trivial
├── receiver.rs   Local HTTP receiver (Click'n'Load), 127.0.0.1 only
└── scripts.rs    Browser console scripts for TikTok and Douyin, with an on-page HUD
```

**Stack**: [egui/eframe](https://github.com/emilk/egui) for the UI (GPU-accelerated, pure Rust), [tokio](https://tokio.rs) + [reqwest](https://github.com/seanmonstar/reqwest) with rustls for the async engine.

See [SECURITY.md](SECURITY.md) for the threat model and security audit.

### Adding a language

In `src/i18n.rs`: add a variant to the `Lang` enum, include it in `Lang::ALL` and `label()`, and add the column to each `entry!`. The Settings selector picks it up automatically.

## Contributing

Issues and pull requests are welcome. If you're reporting a download failure, please include the full error message — hover over the status pill in the Errors tab to copy it.

## Legal notice

This is a tool for personal use: downloading content you already have legitimate access to, backing up your own posts, or archiving material with permission.

Respect each platform's terms of service and creators' copyright. **Responsibility for how it's used lies with the user.**

## License

[MIT](LICENSE) © 2026 Eric V. Gramunt
