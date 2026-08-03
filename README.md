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
- **File hosts, resolved natively.** Pixeldrain, GoFile and MediaFire links are resolved to their real CDN URLs in pure Rust — no extra binary, no Python — and then downloaded through the native HTTP engine with full resume. A folder link expands into one row per file. For hosts that actively fight scrapers (Bunkr, Cyberdrop…), an **optional** cyberdrop-dl engine can be installed from Settings; it's the only thing here that pulls in Python, and only if you choose to.
- **Native BitTorrent** (magnet + `.torrent`) in its own **Torrent** tab, built on the embedded [librqbit](https://github.com/ikatson/rqbit) engine (Rust, Apache-2.0): DHT so magnets work trackerless, UDP/HTTP trackers, uTP, PEX and UPnP port-forwarding. No external client, no Java, no daemon — it compiles into the single binary. Each torrent shows live progress, download speed, connected peers, ETA and uploaded amount; you can set a **per-download folder** and **download/upload speed limits**, and pause/resume/remove. Downloading via torrent also seeds, so the tab carries a clear legal reminder.
- **Always maximum quality.** For ByteDance images (TikTok/Douyin) it detects the CDN's `~tplv-` processing template — which applies watermarks and downscaling — and requests the unprocessed original first. The difference is real: from a recompressed thumbnail to **2160×2880, watermark-free**.
- **Adaptive video quality**: with ffmpeg installed it merges separate video and audio streams (1080p+ on YouTube); without it, it falls back to the best pre-merged file instead of failing.
- **Bilibili tuned for maximum bitrate.** Bilibili publishes every resolution twice — once in AVC, once in HEVC — and the AVC stream often carries far more bitrate (4174k vs 2503k at 1080p). The format sorter prefers resolution, then fps, then bitrate, ignoring the default codec preference. Note that 720p and above require cookies, and 4K / 1080p60 additionally require a premium (大会员) account.
- Queue with **concurrent downloads** (1–8), per-file and global progress and speed.
- **Real pause and resume** using HTTP Range over `.part` files. Pause also terminates engine subprocesses and their whole process tree — yt-dlp and gallery-dl are PyInstaller bundles that run Python in a grandchild process, so killing only what you spawned leaves an orphan downloading.
- **Resumable galleries.** gallery-dl keeps an archive of what it already fetched, so when a site cuts you off halfway through a 400-post profile, *Retry* picks up where it stopped instead of starting over and hitting the same wall forever. Clearable from Settings.
- Exponential backoff retries and per-site request spacing — Instagram gets 6–12 s between requests, everything else 1.5 s.

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
| **cyberdrop-dl** *(optional)* | Hard file hosts: Bunkr, Cyberdrop… | installed via `uv` (needs Python) |

Pixeldrain, GoFile and MediaFire need **no engine at all** — they're resolved natively in Rust. cyberdrop-dl is the only optional, Python-based engine, off by default.

Each binary is verified by running it after download; if it doesn't respond, it's deleted.

### Interface

- **Three themes**: *Classic* (dark with pink accent), *Sober* (slate grey, understated — for shared screens) and *Hot Pink* (vivid pink with soft background glows). Switchable instantly, no restart.
- **Custom background image** for the main panel, with independent **strength** and **gaussian blur** sliders. The sidebar deliberately stays solid so the menu is always readable.
- Dark theme, sidebar navigation, stat cards and an animated *gloss* hover effect that picks up the active theme's accent.
- **Thumbnails in the queue.** Posts captured from TikTok or Douyin show their cover art next to the filename, fetched lazily and only for rows actually on screen.
- **Magnet link handler**: register the app so clicking a magnet in your browser adds it straight to the Torrent tab. If the app is already open, the link goes to that window instead of spawning a second one.
- **Middle-click autoscroll**, like a browser: click the wheel to anchor, then distance from the anchor sets direction and speed.
- **Live cookie indicator** in the sidebar. The app disables unreadable cookies automatically, and without this you had no way of knowing you were downloading anonymously.
- Errors explain themselves: a raw `401 Unauthorized` from Instagram becomes *"enable cookies in Settings and press Retry"*, with the original message underneath.
- **English and Spanish**, with automatic system-language detection and hot switching.
- CJK font support: Chinese, Japanese and Korean titles render correctly.

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
| Grab a Bilibili channel | **Profile** tab → paste `space.bilibili.com/UID/video` → analyze |
| Download a torrent / magnet | **Torrent** tab → paste the magnet or pick a `.torrent` |
| Click magnet links in the browser | **Settings** → *Open magnet links with Todo Downloader*, then pick it in Windows *Default apps → MAGNET* |
| Grab a Pixeldrain / GoFile / MediaFire link | Just paste it — resolved natively, folders expand into individual files |
| Grab from Bunkr / Cyberdrop | Install cyberdrop-dl once in **Settings**, then paste the link |
| Queue a list of links | **Add links** or **Import TXT/JSON** |

### About cookies

Some sites (Instagram, Douyin, Weibo, Bilibili above 480p, age-restricted content) require a signed-in session. **Instagram in particular will not list a full profile without one** — it serves the first few dozen posts anonymously and then returns `401 Unauthorized`.

> ⚠️ **Chrome 127+, Edge, Brave and Opera encrypt cookies with App-Bound Encryption**: no external tool can read them, not even with the browser closed. This is not a bug in this application.

Two alternatives that do work:

1. **Firefox** — not affected. Sign in there, then Settings → Cookies → `firefox`.
2. **A `cookies.txt` file** (most reliable) — export it with an extension like *Get cookies.txt LOCALLY* and select it in Settings. Works with any browser, ignores the encryption entirely, and takes priority over browser extraction.

Check the sidebar: it reads **● cookies enabled** or **○ no cookies**. If it flips back to grey on its own, the cookies could not be read.

If a site still rejects a valid session, check whether a **VPN** is involved. Sign in and download from the same IP — Instagram invalidates sessions that jump between addresses.

## Known limitations

Being upfront about these:

- **Douyin profiles cannot be enumerated** by yt-dlp or gallery-dl — no profile extractor exists for that site. That is precisely why the **Capture** tab exists: it solves the case from inside the browser.
- Direct CDN links **expire within hours**. If you capture thousands of files, the earliest ones may expire before their turn comes. Download in batches.
- Pause terminates the engine subprocess tree within ~150 ms, so a half-written file may be left behind. For galleries the archive records what completed, so resuming continues cleanly.
- **Instagram is the most hostile site supported.** Even with valid cookies it may return `401` mid-profile — there are long-standing upstream issues about it. The resumable archive is a mitigation, not a cure: retry in batches. Heavy scraping can also get an account flagged, so use one you don't mind risking.
- The binary is **not code-signed**, so SmartScreen or your antivirus may warn about it. It's a reputation-based false positive — the hash and the full source are published here.
- Automatic ffmpeg installation is Windows-only; on Linux and macOS use your package manager.
- The Torrent tab shows **connected peers**, not a swarm seeder/leecher split — librqbit's aggregate stats don't expose that cleanly, and per-peer inspection isn't worth the fragility. Peer GeoIP (countries) is intentionally omitted: it needs a multi-MB database and is unreliable behind VPNs.
- BitTorrent opens a listening port; your firewall may prompt on first use. Torrent speed limits are applied when the session starts (restart to change them mid-session).
- **Bilibili requires ffmpeg**, always — it only serves DASH, so video and audio arrive as separate streams that must be merged. The app says so explicitly instead of letting yt-dlp fail cryptically. Bilibili also returns HTTP 412 if channel pagination goes too fast; requests are spaced out to avoid it.

## Architecture

```
assets/           App icon (.ico embedded in the Windows .exe, .png for the window)
build.rs          Embeds the icon and version metadata into the Windows binary
src/
├── main.rs       UI (egui) + download engine (tokio/reqwest)
├── hosters.rs    Native resolvers for open-API file hosts (Pixeldrain, GoFile, MediaFire)
├── torrents.rs   BitTorrent engine facade over librqbit (magnet + .torrent)
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
