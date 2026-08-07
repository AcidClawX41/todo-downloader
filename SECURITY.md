# Security — Todo Downloader

Summary of the security audit and the application's threat model. Last reviewed for v1.6.5.

## Communication channels

All network traffic uses **HTTPS**. There is no code path that accepts invalid certificates. Any `http://` link added to the queue is **automatically rewritten to `https://`** before downloading: the application never transmits in the clear. Helper engines are downloaded exclusively from fixed GitHub Releases URLs (`github.com/yt-dlp/yt-dlp`, `github.com/gdl-org/builds`, `github.com/yt-dlp/FFmpeg-Builds`) over TLS.

**On the TLS backend, precisely.** `Cargo.toml` requests reqwest with `rustls-tls` and without default features, but Cargo features are *additive and unified across the dependency graph*: `librqbit` enables `reqwest/default-tls`, so the resolved feature set contains **both** `rustls-tls` and `default-tls`. When both are present reqwest's `ClientBuilder` defaults to **native-tls**, which means the platform TLS stack — Schannel on Windows, Secure Transport on macOS, OpenSSL on Linux — not rustls. Earlier revisions of this document claimed "rustls, no system OpenSSL"; that was inaccurate and has been corrected. This is a documentation fix, not a vulnerability: native-tls validates certificates against the platform trust store and rejects invalid ones exactly as rustls would. Pinning the backend explicitly with `.use_rustls_tls()` is under consideration, with the caveat that rustls here is built against bundled webpki roots rather than the system trust store, which would break users behind a corporate TLS-inspecting proxy or with a private CA.

## Attack surface and mitigations

**Command injection**: yt-dlp and gallery-dl are invoked via `Command` with arguments passed as an array — nothing ever goes through a shell, so shell injection is impossible. Against *argument injection* (a malicious "URL" starting with `-` trying to sneak in as a flag, e.g. `--exec`), every URL is passed after the `--` separator, which terminates the option list. In addition, only strings beginning with `http` are accepted.

**Path traversal**: all filenames and author folder names go through `sanitize()`, which strips path separators, control characters and wildcards, trims trailing dots, and neutralizes Windows reserved device names (CON, NUL, COM1…). A malicious video title cannot write outside the destination folder.

**Native file-host resolvers**: Pixeldrain, GoFile and MediaFire are resolved in-process with reqwest — no external binary. Only official API endpoints and the hoster's own page are contacted, all over HTTPS. Resolved links are downloaded through the same native HTTP path as everything else (which only ever *writes* the response to disk, never executes it). GoFile requires a per-download cookie (a guest-account token); it is obtained fresh from GoFile's API, sent only to GoFile's own CDN, and never persisted. The MediaFire resolver parses HTML with fixed regexes and decodes an optional base64 link — it does not evaluate any script from the page.

**MEGA public links (native engine)**: MEGA stores ciphertext it cannot read; the decryption key travels only in the URL fragment (`#...`), which is never transmitted to any server. This application preserves that property: the key is parsed locally, used locally, and **never sent to MEGA's API**. It is redacted from logs, from error messages and from `Debug` output — `FileKey` prints as `[REDACTED]` and key buffers are zeroed on drop. Full public links are not persisted beyond the queue entry needed to retry.

Decryption is AES-128-CTR, which provides confidentiality but **not** authenticity: a flipped byte in transit would silently produce a flipped byte on disk. The engine therefore recomputes MEGA's chunked condensed MAC over the complete file and compares it, in constant time, against the value embedded in the link key. Verification happens **before** the `.part` file is renamed, so a corrupt, truncated or wrong-key download can never appear under the final filename — on mismatch the partial file is set aside as `.part.corrupt`. Filenames arrive encrypted in the node attributes and are passed through `sanitize()` before touching disk; folder paths are rebuilt component by component with `..`, separators, drive prefixes and control characters rejected, and parent-chain depth is bounded so a hostile response cannot loop.

No MEGA account credentials are requested or stored. There is no login, no session persistence and no device fingerprinting: the engine needs none of them for public links, and the only dependency added is `aes` (MIT OR Apache-2.0), a pure-Rust block cipher. Transfer URLs are temporary and re-requested on expiry, with bounded retries so a dead link cannot spin forever. HTTP 509 (quota) and API `-3`/`-4` (congestion, rate limit) are surfaced as distinct, non-retrying errors rather than an endless loop.

**BitTorrent engine (librqbit)**: embedded as a Rust library, not a subprocess. It opens a listening port and participates in DHT and the swarm, which is inherently more network-exposed than the rest of the app — this is intrinsic to how BitTorrent works, not a flaw. Its session is created **lazily**, only when you add the first torrent, so if you never use the tab no port is opened and no DHT traffic occurs. Downloading a torrent also **uploads** (seeds) to peers; the tab states this plainly, since it is both a bandwidth and a legal consideration. Torrents download to a dedicated `Torrents/` subfolder. As with any download, content is written to disk and never executed.

**A note on how this section changed**: earlier releases stated flatly that no
credentials were requested. That stopped being true in v1.6.2 and the claim has
been rewritten rather than quietly softened. The principle that survived is the
one that matters: **passwords are never written to disk**.

**Reading browser cookies**: the native engines can read Firefox's
`cookies.sqlite` to reuse a session you already have open. The database is
copied to a temporary file before being opened — Firefox locks it while running
— and the copy, along with any `-wal`/`-shm` companions, is deleted immediately
afterwards. Nothing is decrypted, no password is ever requested, and cookies are
filtered by exact domain match before use, so a session for one site is never
sent to another. Chromium-based browsers are deliberately not supported here, though the reason
differs by platform and the interface used to state it too broadly. **On
Windows** their cookies are encrypted with App-Bound Encryption, which requires
the read to come from the browser process itself — genuinely out of reach. **On
Linux and macOS** they are protected by gnome-keyring, kwallet or the Keychain,
which are readable with the user's permission; yt-dlp and gallery-dl do exactly
that. This module still covers only Firefox: supporting three key stores to save
one file export is not a trade worth making, and `cookies.txt` works everywhere.
The warning in Settings is now shown only on Windows, where it is true.

Note a real limitation of that mechanism, found while testing: Firefox writes
only cookies **with an expiry date** to `cookies.sqlite`. Session cookies live
in memory and are never on disk, so a login that issues one cannot be picked up
this way by any external tool. That is precisely why the in-app sign-in exists.

**Browser capture scripts**: the Capture tab hands you JavaScript to paste into
your own browser's console. Read it before you do — that is why the tab shows
the full source rather than a download link. The scripts fetch pages from the
site you are already on, extract media URLs and POST them to the local receiver;
they do not read cookies, do not touch other origins and do not persist
anything. The receiver accepts only `http(s)` URLs and merely queues downloads.
Chrome blocks pages from reaching `127.0.0.1`, in which case the script saves a
JSON file instead — the same file the application can import.

**SQLite**: `rusqlite` is compiled with the `bundled` feature, so SQLite is
built from source shipped inside the crate rather than linked against a system
library. It is used only to read the cookie database; nothing is written and no
database is created.

**Optional cyberdrop-dl engine**: opt-in from Settings, off by default. It is the only component that pulls in Python: installation runs the official `uv` installer (`astral.sh`) followed by `uv tool install cyberdrop-dl-patched`. If you never enable it, no Python is downloaded and nothing changes. When enabled, it runs as a subprocess under the same pause/kill-tree control as the other engines.

**Downloaded binaries (supply chain)**: after downloading yt-dlp, gallery-dl or ffmpeg, a verification run is performed (`--version` / `-version`); if the binary does not respond correctly it is **deleted** and the user is notified. ffmpeg arrives as an archive, and **only** `ffmpeg.exe` and `ffprobe.exe` are extracted, filtered by exact path (`*/bin/ffmpeg.exe`); no other archive entry is written to disk, and the temporary zip is removed afterwards. The source is the official build maintained by the yt-dlp team itself. Files are written as `.part` first and renamed atomically. Known limitation: release signatures are not verified (GitHub does not publish uniform signatures for these projects); trust rests on TLS + github.com. Keep Windows Defender or your AV enabled as an additional layer.

**Clipboard (LinkGrabber)**: read locally every 900 ms, *only when enabled*. Only URLs from known sites are extracted (or any URL, if the user explicitly allows it); clipboard content is **never logged, persisted or transmitted**. It can be turned off in Settings.

**Browser cookies**: opt-in feature. Cookies are read by yt-dlp/gallery-dl directly and travel only to the destination site over TLS; this application never touches, stores or forwards them.

Cookies are also **not sent to sites that do not need them**. Public content is fetched anonymously on the first attempt, and the session is only attached on retry if the site's error genuinely asks for authentication (login required, private, age-restricted, members-only, 401/403). Sites that refuse to list anything without a session — Instagram, Weibo, the social networks — still get them from the start. Beyond reducing how widely your session is exposed, this is what makes public YouTube downloads work at all: with account cookies present, yt-dlp switches to a client that requires a PO Token, and without a PO Token provider every format is discarded and the download fails with `Requested format is not available` ([yt-dlp#16569](https://github.com/yt-dlp/yt-dlp/issues/16569)).

**Data at rest**: persisted settings are mostly non-sensitive (paths, booleans, browser name), with one exception that must be stated plainly: **if you enter Booru API credentials, the username and key are saved in the settings file in plaintext.** They are masked in the interface, but eframe's settings store is not encrypted. The file lives in the application's data folder inside your user profile, so it is protected by the profile's own ACL and nothing more. If that is not acceptable for you, leave the field empty and use the Booru sites that work anonymously — only Gelbooru actually requires credentials. **V2PH sign-in**, added in v1.6.2, is the second exception. V2PH shows only the first ten photos of an album to visitors, so the application offers an in-app sign-in. **The password is never stored.** It is read from the field, sent in a single request to V2PH's own login form, and destroyed when that request returns — it is never written to the settings file, a log or an error message. What is persisted is the **session cookie the site returns**, which is what a browser stores too: a revocable credential, not a reusable secret. Signing out deletes it. The same plaintext caveat as the Booru keys applies to that cookie, and it is worth understanding what it means — someone with read access to your user profile could reuse that session until it expires or you sign out.

No payment or account data beyond the above exists anywhere in the application. The gallery-dl download archive (`descargados.sqlite3`, in the application's own data folder) holds only opaque per-site identifiers of already-fetched items, so retries can resume; it contains no URLs, credentials or file contents, and can be deleted at any time from Settings.

**Magnet protocol handler**: opt-in from Settings. Registration writes to `HKEY_CURRENT_USER` only — no administrator rights, no machine-wide changes — and is done through `reg.exe` rather than adding a registry crate. Windows still protects the *actual* default with its signed `UserChoice` key, which no application can forge; the app therefore only publishes its capabilities so the user can pick it in Settings. When a magnet is clicked and an instance is already running, the link is handed to it through the same localhost-only receiver and the second process exits immediately.

**Booru credentials**: optional, and only Gelbooru actually requires them. The key field is masked in the UI and no credential is ever written to a log or an error message.

They are **never passed as process arguments**. Command lines are readable by any process running as the same user (`wmic process get commandline`, Task Manager's *Command line* column, `ps aux`), so instead the app writes a minimal config file, hands it to gallery-dl with `-c` — which *merges* with the user's own gallery-dl configuration rather than replacing it — and **deletes it as soon as the search finishes**, success or failure. On Unix the file is created with mode `0600` before any content is written, so it never exists with permissive bits. On Windows it lives in the app's data folder inside the user profile, inheriting an ACL restricted to that user. A leftover file from an abrupt shutdown is removed at startup.

This leaves a small, bounded exposure — the credentials are on disk for the seconds a search takes — which is strictly better than being visible in the process list, and far better than storing them permanently in plaintext.

**Booru search**: runs gallery-dl in `-j --no-download` mode, which only dumps metadata. Search results are parsed defensively — a malformed or hostile response yields zero posts and a readable error rather than a crash. Thumbnails are capped at 4 MiB, decoded off the async pool and kept in memory only.

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
