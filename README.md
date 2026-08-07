<div align="center">

# ⬇️ Todo Downloader

**A lightweight, bloat-free desktop download manager.**
Written in Rust. Single portable executable — no installer, no runtime, no Java.

[![Build](https://github.com/AcidClawX41/todo-downloader/actions/workflows/build.yml/badge.svg)](https://github.com/AcidClawX41/todo-downloader/actions/workflows/build.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-2021-orange?logo=rust)
![egui](https://img.shields.io/badge/GUI-egui%200.28-blue)
![Platforms](https://img.shields.io/badge/Windows%20%7C%20Linux%20%7C%20macOS-lightgrey)

</div>

---

## What is this?

A download manager in the spirit of JDownloader2 — but **without Java, without an installer, without adware and without telemetry**. A single ~9 MB executable that starts instantly.

It started as a tool to grab full TikTok and Douyin profiles in maximum quality, and grew into something that handles 1000+ sites.

**New in v1.6.5:**

- **Audio and video are merged on Linux and macOS again** — the application was passing an empty `--ffmpeg-location` to yt-dlp, so the merger was never found and the two streams stayed apart. Windows was never affected.
- **A bitrate setting** — *Settings → Downloads → Prefer bitrate over codec efficiency*, on by default. Same resolution and same fps, but the highest-bitrate encode of them wins instead of the most compact one.
- **Galleries load the whole profile** — the background chain now runs to the end instead of stopping after four pages. *Load more* is still there.
- **Images and videos counted separately** in the gallery header, over the whole list rather than the filtered view.
- **Engine messages follow the selected language** — errors produced outside the interface were hardcoded in Spanish and now switch with it.

**Also in v1.6.2 and v1.6.0:**

- **Native MEGA.nz public-link downloads** — decrypted on your machine, resumable, and verified against MEGA's own file MAC before the download is considered complete.
- **Preview grids for Instagram, Weibo, TikTok, Bilibili and V2PH** — analyze a profile, see the actual photos and video covers, and queue only the ones you want.
- **Native V2PH extractor** — full albums and whole model profiles, in original quality, with no external engine, plus a browser-side script for when the site pushes back.
- **Sign-in from Settings, native Firefox cookie reading and a User-Agent field** — three ways to give the application a session, and honest documentation of what each one cannot do.

## Features

### Downloading

- **Universal**: direct file links over native HTTP, and pages from 1000+ sites (YouTube, TikTok, Instagram, X, Reddit, Twitch, Weibo, Bilibili…) through the built-in engines.
- **File hosts, resolved natively.** Pixeldrain, GoFile and MediaFire links are resolved to their real CDN URLs in pure Rust — no extra binary, no Python — and then downloaded through the native HTTP engine with full resume. A folder link expands into one row per file. For hosts that actively fight scrapers (Bunkr, Cyberdrop…), an **optional** cyberdrop-dl engine can be installed from Settings; it's the only thing here that pulls in Python, and only if you choose to.
- **MEGA.nz public links, natively.** Public file and folder links are downloaded by an engine compiled into the binary — no MEGAcmd, no browser automation, no helper executable. MEGA never sees your decryption key: it lives only in the URL fragment, so the file is decrypted **on your machine** while it streams to disk. Full `.part` resume works because AES-CTR is seekable, and every download is checked against MEGA's own file MAC before the final filename is created — a corrupt or truncated transfer never appears as a completed file. A folder link expands into one queue row per file, each with its own progress, pause and error. **Account login is not supported and no MEGA credentials are ever requested or stored.**
- **Native BitTorrent** (magnet + `.torrent`) in its own **Torrent** tab, built on the embedded [librqbit](https://github.com/ikatson/rqbit) engine (Rust, Apache-2.0): DHT so magnets work trackerless, UDP/HTTP trackers, uTP, PEX and UPnP port-forwarding. No external client, no Java, no daemon — it compiles into the single binary. Each torrent shows live progress, download speed, connected peers, ETA and uploaded amount; you can set a **per-download folder** and **download/upload speed limits**, and pause/resume/remove. Downloading via torrent also seeds, so the tab carries a clear legal reminder.
- **Always maximum quality.** For ByteDance images (TikTok/Douyin) it detects the CDN's `~tplv-` processing template — which applies watermarks and downscaling — and requests the unprocessed original first. The difference is real: from a recompressed thumbnail to **2160×2880, watermark-free**.
- **Adaptive video quality**: with ffmpeg installed it merges separate video and audio streams (1080p+ on YouTube); without it, it falls back to the best pre-merged file instead of failing.
- **Bilibili tuned for maximum bitrate.** Bilibili publishes every resolution twice — once in AVC, once in HEVC — and the AVC stream often carries far more bitrate (4174k vs 2503k at 1080p). The format sorter prefers resolution, then fps, then bitrate, ignoring the default codec preference. Note that 720p and above require cookies, and 4K / 1080p60 additionally require a premium (大会员) account.
- Queue with **concurrent downloads** (1–8), per-file and global progress and speed.
- **Real pause and resume** using HTTP Range over `.part` files. Pause also terminates engine subprocesses and their whole process tree — yt-dlp and gallery-dl are PyInstaller bundles that run Python in a grandchild process, so killing only what you spawned leaves an orphan downloading.
- **Resumable galleries.** gallery-dl keeps an archive of what it already fetched, so when a site cuts you off halfway through a 400-post profile, *Retry* picks up where it stopped instead of starting over and hitting the same wall forever. Clearable from Settings.
- Exponential backoff retries and per-site request spacing — Instagram gets 6–12 s between requests, everything else 1.5 s.

### Booru browser

A dedicated **Booru** tab for Danbooru, Safebooru, AIBooru, yande.re, Konachan, e621 and Gelbooru. Search by tags, review results as a **thumbnail grid**, click to select, and queue only what you want — always the **original file**, downloaded through the native HTTP engine with resume.

- Filter locally by **minimum width** and **rating** without re-querying the site.
- Each tile shows resolution (highlighted when ≥1920 px), format and file size.
- Pagination, select-all/none, and thumbnails carried through to the download queue.

Listing uses `gallery-dl -j`, which dumps metadata **without downloading**. Danbooru, Gelbooru and Moebooru all expose different, shifting APIs; gallery-dl already maintains an extractor per site, so reimplementing them in Rust would be permanent maintenance for no gain. Parsing is deliberately tolerant — field names differ per site (`image_width` vs `width`, some booru APIs return integers **as strings**, e621 nests them under `file`).

> **Gelbooru requires API credentials** (`AuthRequired` without them). Add them in *Settings → Booru accounts*; the key field is masked. Everything else works anonymously.

### V2PH albums and profiles

Paste a V2PH album or model URL into the **Profile** tab and pick what to
download from the preview grid. The extractor is written in Rust: the site
serves plain server-rendered HTML with the original image URLs in the markup,
so no external engine, browser automation or Python is involved.

- An album is split into pages of ten photos; all of them are walked, so a
  38-photo album lists 38 photos.
- On a model, agency, category or country page, each grid page is one complete
  album.
- **Past the tenth photo of an album V2PH requires a session.** Point
  *Settings → Cookies* at a `cookies.txt` exported from a signed-in browser.
- V2PH also limits **how many albums an account may open per day**. Nothing in
  the application can raise that; already-opened albums can be revisited freely.

If the site starts answering `403` — it rate-limits bursts, and an analysis is
several requests — the **Capture** tab has a V2PH script that does the listing
from inside your own browser instead. See below.

### Capturing links

- **LinkGrabber**: watches the clipboard and queues URLs as you copy them, just like JDownloader.
- **Profile view with previews**: paste a profile URL, analyze it, and pick what to download from a **thumbnail grid** instead of a list of titles. TikTok and Bilibili show each post's cover; Instagram and Weibo show the real photos and video covers with resolution, position in the carousel (`3/10`), format and date. Pinterest and similar sites are still downloaded whole — no extractor exposes a listing worth previewing.
- **Browser capture (Click'n'Load)**: a local HTTP receiver accepts links captured by a script running inside the page itself. This solves what no external tool can, because the script inherits your session, your address and your browser's own TLS fingerprint. Scripts are provided for:
  - **Douyin** profiles, which no extractor can enumerate.
  - **TikTok** profiles, as an alternative to the API path.
  - **V2PH**, for when the site rate-limits the application. The browser walks the album and hands the URLs over; downloading is unaffected because the image CDN is not the part that pushes back.
  
  Chrome blocks pages from reaching `127.0.0.1`, so there the script falls back to saving a JSON file you import from *Downloads → Import TXT/JSON*. Firefox delivers directly.
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

The binary lands in `target/release/`.

Run the tests first if you are changing anything:

```bash
cargo test
```

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
| V2PH keeps returning 403 | **Capture** tab → V2PH script → paste into the album's console |
| Grab a Bilibili channel | **Profile** tab → paste `space.bilibili.com/UID/video` → analyze |
| Browse and grab booru art | **Booru** tab → pick a site → tags → select thumbnails → queue |
| Download a torrent / magnet | **Torrent** tab → paste the magnet or pick a `.torrent` |
| Click magnet links in the browser | **Settings** → *Open magnet links with Todo Downloader*, then pick it in Windows *Default apps → MAGNET* |
| Grab a Pixeldrain / GoFile / MediaFire link | Just paste it — resolved natively, folders expand into individual files |
| Grab from Bunkr / Cyberdrop | Install cyberdrop-dl once in **Settings**, then paste the link |
| Queue a list of links | **Add links** or **Import TXT/JSON** |

### About cookies

The native engines can read **Firefox's** cookie database directly, so V2PH
works with *Use browser cookies* selected — no manual export. Firefox stores
cookies unencrypted; Chromium browsers tie them to your Windows account (and,
since Chrome 127, to the browser process itself), so for those a `cookies.txt`
is still required. Only cookies whose domain matches the site being fetched are
ever sent, compared structurally so that `v2ph.com.attacker.net` is not treated
as `v2ph.com`.


Some sites (Instagram, Douyin, Weibo, Bilibili above 480p, age-restricted content) require a signed-in session. **Instagram in particular will not list a full profile without one** — it serves the first few dozen posts anonymously and then returns `401 Unauthorized`.

Cookies are sent **only where they are needed**. Public content is tried anonymously first, and your session is attached on retry only if the site actually asks for it. This is not just hygiene: sending account cookies to a public YouTube video makes yt-dlp switch to a client that demands a PO Token, and without one *every* format is dropped and the download dies with `Requested format is not available`. Instagram, Weibo and the social networks still get cookies from the first request, because they list nothing without a session.

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
- **Reading cookies straight from the browser only covers Firefox**, whose cookie database is unencrypted. It also only sees cookies **with an expiry date** — session cookies live in memory and are never on disk, so a login that issues one cannot be picked up this way by any external tool. Use a `cookies.txt` in that case. Chromium browsers are unreadable on Windows (App-Bound Encryption); on Linux and macOS yt-dlp and gallery-dl can read them, with a Keychain prompt on macOS.
- **V2PH gates its login page behind Cloudflare**, so the in-app sign-in cannot work there and says so. Album pages are not challenged; a `cookies.txt` is the supported route.
- The Torrent tab shows **connected peers**, not a swarm seeder/leecher split — librqbit's aggregate stats don't expose that cleanly, and per-peer inspection isn't worth the fragility. Peer GeoIP (countries) is intentionally omitted: it needs a multi-MB database and is unreliable behind VPNs.
- BitTorrent opens a listening port; your firewall may prompt on first use. Torrent speed limits are applied when the session starts (restart to change them mid-session).
- **Bilibili requires ffmpeg**, always — it only serves DASH, so video and audio arrive as separate streams that must be merged. The app says so explicitly instead of letting yt-dlp fail cryptically. Bilibili also returns HTTP 412 if channel pagination goes too fast; requests are spaced out to avoid it.

## Architecture

```
assets/           App icon (.ico embedded in the Windows .exe, .png for the window)
build.rs          Embeds the icon and version metadata into the Windows binary
src/
├── main.rs       UI (egui) + download engine (tokio/reqwest)
├── gallery.rs    Gallery listing for the Instagram/Weibo preview grid
├── hosters.rs    Native resolvers for open-API file hosts (Pixeldrain, GoFile, MediaFire)
├── v2ph.rs       Native V2PH album and profile extractor (plain HTML, no engine)
├── cookies.rs    Reads Firefox's cookie database for the native engines
├── mega/         Native MEGA.nz engine: link parsing, crypto, API, folders, download
├── booru.rs      Booru search over gallery-dl's JSON dump, tolerant across APIs
├── torrents.rs   BitTorrent engine facade over librqbit (magnet + .torrent)
├── i18n.rs       EN/ES translations — adding languages is trivial
├── receiver.rs   Local HTTP receiver (Click'n'Load), 127.0.0.1 only
└── scripts.rs    Browser console scripts for TikTok and Douyin, with an on-page HUD
```

**Stack**: [egui/eframe](https://github.com/emilk/egui) for the UI (GPU-accelerated, pure Rust), [tokio](https://tokio.rs) + [reqwest](https://github.com/seanmonstar/reqwest) for the async engine. TLS goes through the platform stack (Schannel / Secure Transport / OpenSSL) rather than rustls — see [SECURITY.md](SECURITY.md) for why, and why saying otherwise would be inaccurate.

See [SECURITY.md](SECURITY.md) for the threat model and security audit.

### Adding a language

In `src/i18n.rs`: add a variant to the `Lang` enum, include it in `Lang::ALL` and `label()`, and add the column to each `entry!`. The Settings selector picks it up automatically.

## Contributing

Issues and pull requests are welcome. If you're reporting a download failure, please include the full error message — hover over the status pill in the Errors tab to copy it.

## Support

Todo Downloader is free, open source, ad-free and telemetry-free, and it will stay that way — **no feature is behind a paywall**. If it saves you time, there's a *Support this project* panel in **Settings** with links to Ko-fi, PayPal and GitHub Sponsors.

Those buttons simply open your browser. The application contains no payment SDK, no API keys and no credentials, and never sees payments or banking details.

## Legal notice

This is a tool for personal use: downloading content you already have legitimate access to, backing up your own posts, or archiving material with permission.

Respect each platform's terms of service and creators' copyright. **Responsibility for how it's used lies with the user.**

## License

[GPL-3.0-or-later](LICENSE) © 2026 Eric V. Gramunt

> **Todo Downloader v1.5.0 and later are licensed under GPL-3.0.**
> Versions up to and including v1.4.0 were released under the MIT License and remain available under those terms — see [LICENSE-HISTORY.md](LICENSE-HISTORY.md) for the full picture, including third-party components.

If you distribute this program or a modified version of it, GPL-3.0 requires you to pass on the same freedoms: the recipients must get the source code and the same rights you had.
