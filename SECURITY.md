# Security — Todo Downloader

Summary of the v1.0.0 security audit and the application's threat model.

## Communication channels

All network traffic uses **HTTPS with rustls** (a pure-Rust TLS implementation — no system OpenSSL). There is no code path that accepts invalid certificates. Any `http://` link added to the queue is **automatically rewritten to `https://`** before downloading: the application never transmits in the clear. Helper engines are downloaded exclusively from fixed GitHub Releases URLs (`github.com/yt-dlp/yt-dlp`, `github.com/gdl-org/builds`, `github.com/yt-dlp/FFmpeg-Builds`) over TLS.

## Attack surface and mitigations

**Command injection**: yt-dlp and gallery-dl are invoked via `Command` with arguments passed as an array — nothing ever goes through a shell, so shell injection is impossible. Against *argument injection* (a malicious "URL" starting with `-` trying to sneak in as a flag, e.g. `--exec`), every URL is passed after the `--` separator, which terminates the option list. In addition, only strings beginning with `http` are accepted.

**Path traversal**: all filenames and author folder names go through `sanitize()`, which strips path separators, control characters and wildcards, trims trailing dots, and neutralizes Windows reserved device names (CON, NUL, COM1…). A malicious video title cannot write outside the destination folder.

**Downloaded binaries (supply chain)**: after downloading yt-dlp, gallery-dl or ffmpeg, a verification run is performed (`--version` / `-version`); if the binary does not respond correctly it is **deleted** and the user is notified. ffmpeg arrives as an archive, and **only** `ffmpeg.exe` and `ffprobe.exe` are extracted, filtered by exact path (`*/bin/ffmpeg.exe`); no other archive entry is written to disk, and the temporary zip is removed afterwards. The source is the official build maintained by the yt-dlp team itself. Files are written as `.part` first and renamed atomically. Known limitation: release signatures are not verified (GitHub does not publish uniform signatures for these projects); trust rests on TLS + github.com. Keep Windows Defender or your AV enabled as an additional layer.

**Clipboard (LinkGrabber)**: read locally every 900 ms, *only when enabled*. Only URLs from known sites are extracted (or any URL, if the user explicitly allows it); clipboard content is **never logged, persisted or transmitted**. It can be turned off in Settings.

**Browser cookies**: opt-in feature. Cookies are read by yt-dlp/gallery-dl directly and travel only to the destination site over TLS; this application never touches, stores or forwards them.

**Data at rest**: persisted settings contain no secrets (paths, booleans, browser name). No credentials or tokens are stored.

**Local receiver (browser capture)**: the Click'n'Load feature opens a minimal HTTP endpoint with these restrictions — bound **exclusively to 127.0.0.1** (unreachable from the network, not even from another machine on the LAN), disableable from Settings, request body capped at 8 MiB, and **only `http://` or `https://` strings are accepted**: anything else (`javascript:`, `file:`, local paths) is discarded. Received data is only queued as a download; it is never executed or evaluated. The CORS header is permissive out of necessity — the browser must be able to post from tiktok.com or douyin.com — which is acceptable because the endpoint is unreachable from outside the machine and its only possible action is adding a URL to the queue. Residual risk: a malicious page open in the browser could queue unwanted downloads while the receiver is active; they are visible in the queue and do not start on their own unless auto-start is enabled.

**Resources**: bounded concurrency (1–8), capped retries with backoff, profile analysis limited to 2000 entries, 15 s connection timeout.

## What this application does NOT do

No telemetry, no analytics. It does not execute downloaded content, does not silently self-update, does not touch the Windows registry, and does not require administrator privileges. The only listening port is the local receiver described above, always bound to 127.0.0.1 and disableable.

## Known limitations

- Pausing a running yt-dlp/gallery-dl task does not kill the subprocess (it finishes the file in progress).
- Files downloaded from arbitrary sites are the user's responsibility; the app does not scan them (use your AV).
- No signature verification of the helper binaries (see above).

## Reporting a vulnerability

Open a private issue on the repository or contact the author. — *By Eric V. Gramunt*
