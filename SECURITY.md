# Security — Todo Downloader

Summary of the security audit and the application's threat model. Last reviewed for v1.4.0.

## Communication channels

All network traffic uses **HTTPS with rustls** (a pure-Rust TLS implementation — no system OpenSSL). There is no code path that accepts invalid certificates. Any `http://` link added to the queue is **automatically rewritten to `https://`** before downloading: the application never transmits in the clear. Helper engines are downloaded exclusively from fixed GitHub Releases URLs (`github.com/yt-dlp/yt-dlp`, `github.com/gdl-org/builds`, `github.com/yt-dlp/FFmpeg-Builds`) over TLS.

## Attack surface and mitigations

**Command injection**: yt-dlp and gallery-dl are invoked via `Command` with arguments passed as an array — nothing ever goes through a shell, so shell injection is impossible. Against *argument injection* (a malicious "URL" starting with `-` trying to sneak in as a flag, e.g. `--exec`), every URL is passed after the `--` separator, which terminates the option list. In addition, only strings beginning with `http` are accepted.

**Path traversal**: all filenames and author folder names go through `sanitize()`, which strips path separators, control characters and wildcards, trims trailing dots, and neutralizes Windows reserved device names (CON, NUL, COM1…). A malicious video title cannot write outside the destination folder.

**Native file-host resolvers**: Pixeldrain, GoFile and MediaFire are resolved in-process with reqwest — no external binary. Only official API endpoints and the hoster's own page are contacted, all over HTTPS. Resolved links are downloaded through the same native HTTP path as everything else (which only ever *writes* the response to disk, never executes it). GoFile requires a per-download cookie (a guest-account token); it is obtained fresh from GoFile's API, sent only to GoFile's own CDN, and never persisted. The MediaFire resolver parses HTML with fixed regexes and decodes an optional base64 link — it does not evaluate any script from the page.

**BitTorrent engine (librqbit)**: embedded as a Rust library, not a subprocess. It opens a listening port and participates in DHT and the swarm, which is inherently more network-exposed than the rest of the app — this is intrinsic to how BitTorrent works, not a flaw. Its session is created **lazily**, only when you add the first torrent, so if you never use the tab no port is opened and no DHT traffic occurs. Downloading a torrent also **uploads** (seeds) to peers; the tab states this plainly, since it is both a bandwidth and a legal consideration. Torrents download to a dedicated `Torrents/` subfolder. As with any download, content is written to disk and never executed.

**Optional cyberdrop-dl engine**: opt-in from Settings, off by default. It is the only component that pulls in Python: installation runs the official `uv` installer (`astral.sh`) followed by `uv tool install cyberdrop-dl-patched`. If you never enable it, no Python is downloaded and nothing changes. When enabled, it runs as a subprocess under the same pause/kill-tree control as the other engines.

**Downloaded binaries (supply chain)**: after downloading yt-dlp, gallery-dl or ffmpeg, a verification run is performed (`--version` / `-version`); if the binary does not respond correctly it is **deleted** and the user is notified. ffmpeg arrives as an archive, and **only** `ffmpeg.exe` and `ffprobe.exe` are extracted, filtered by exact path (`*/bin/ffmpeg.exe`); no other archive entry is written to disk, and the temporary zip is removed afterwards. The source is the official build maintained by the yt-dlp team itself. Files are written as `.part` first and renamed atomically. Known limitation: release signatures are not verified (GitHub does not publish uniform signatures for these projects); trust rests on TLS + github.com. Keep Windows Defender or your AV enabled as an additional layer.

**Clipboard (LinkGrabber)**: read locally every 900 ms, *only when enabled*. Only URLs from known sites are extracted (or any URL, if the user explicitly allows it); clipboard content is **never logged, persisted or transmitted**. It can be turned off in Settings.

**Browser cookies**: opt-in feature. Cookies are read by yt-dlp/gallery-dl directly and travel only to the destination site over TLS; this application never touches, stores or forwards them.

**Data at rest**: persisted settings contain no secrets (paths, booleans, browser name). No credentials or tokens are stored. The gallery-dl download archive (`descargados.sqlite3`, in the application's own data folder) holds only opaque per-site identifiers of already-fetched items, so retries can resume; it contains no URLs, credentials or file contents, and can be deleted at any time from Settings.

**Magnet protocol handler**: opt-in from Settings. Registration writes to `HKEY_CURRENT_USER` only — no administrator rights, no machine-wide changes — and is done through `reg.exe` rather than adding a registry crate. Windows still protects the *actual* default with its signed `UserChoice` key, which no application can forge; the app therefore only publishes its capabilities so the user can pick it in Settings. When a magnet is clicked and an instance is already running, the link is handed to it through the same localhost-only receiver and the second process exits immediately.

**Custom background image**: read from a path the user picks in a file dialog, decoded with the `image` crate, downscaled and kept in memory only. A malformed image simply fails to decode and no background is shown; nothing is copied or written elsewhere.

**Thumbnails**: cover images are fetched over HTTPS with the same per-domain Referer logic as downloads, capped at 6 MiB, decoded off the async pool, and kept **in memory only** — never written to disk. Decoding is delegated to the `image` crate; a malformed or hostile image simply fails to decode and the row shows no thumbnail. At most 512 are held at once.

**Local receiver (browser capture)**: the Click'n'Load feature opens a minimal HTTP endpoint with these restrictions — bound **exclusively to 127.0.0.1** (unreachable from the network, not even from another machine on the LAN), disableable from Settings, request body capped at 8 MiB, and **only `http://` or `https://` strings are accepted**: anything else (`javascript:`, `file:`, local paths) is discarded. Received data is only queued as a download; it is never executed or evaluated. The CORS header is permissive out of necessity — the browser must be able to post from tiktok.com or douyin.com — which is acceptable because the endpoint is unreachable from outside the machine and its only possible action is adding a URL to the queue. Residual risk: a malicious page open in the browser could queue unwanted downloads while the receiver is active; they are visible in the queue and do not start on their own unless auto-start is enabled.

**Resources**: bounded concurrency (1–8), capped retries with backoff, profile analysis limited to 2000 entries, 15 s connection timeout.

**User control over subprocesses**: Pause sets a cancellation flag that native HTTP downloads check between chunks and that engine subprocesses are polled against every 150 ms; on cancellation the **entire process tree** is terminated. The tree matters: yt-dlp and gallery-dl ship as PyInstaller "onefile" bundles, so the process you spawn is a bootloader that runs the real Python interpreter in a *grandchild*. Terminating only the bootloader leaves that grandchild orphaned and still downloading — which is exactly what happened before v1.1.0, where the flag was honoured by native downloads only. On Windows the whole tree is killed via `taskkill /T`; on Linux and macOS each engine is spawned in its own process group (`process_group(0)`) and cancellation signals the entire group (SIGKILL to `-pid`), so the Python grandchild is reached on every platform. Not a privilege or disclosure issue, but a loss of user control and a violation of the resource bounds claimed above.

## What this application does NOT do

No telemetry, no analytics. It does not execute downloaded content, does not silently self-update, does not touch the Windows registry, and does not require administrator privileges. The only listening port is the local receiver described above, always bound to 127.0.0.1 and disableable.

## Known limitations

- Pause terminates the engine process tree, but a partially written file may remain on disk.
- Files downloaded from arbitrary sites are the user's responsibility; the app does not scan them (use your AV).
- No signature verification of the helper binaries (see above).

## Reporting a vulnerability

Open a private issue on the repository or contact the author. — *By Eric V. Gramunt*
