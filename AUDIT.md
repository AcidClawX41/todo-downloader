# Todo Downloader — audit and hotfix status

**Last updated:** 2026-08-12 (v1.6.7)
**Baseline:** v1.5.0, publicly released and working.
**Target release:** v1.6.7.

This document tracks what was found, what has been fixed, and what is still open. It is kept current: a finding is only marked fixed once the change compiles and its tests pass.

---

## 1. Verification status

Toolchain used for verification: `cargo 1.97.1`, `rustc 1.97.1`, Linux x86_64.

| Command | Result | Notes |
|---|---|---|
| `cargo check --all-targets` | **pass** | 0 errors |
| `cargo test` | **pass** | 44 tests, 0 failures |
| `cargo fmt --all -- --check` | **fails** | 217 diff hunks (197 in `main.rs`). Pre-existing; CI does not block on it (`continue-on-error: true`) |
| `cargo clippy --all-targets --all-features` | not run | — |
| `cargo build --release` | not run | — |

One warning, pre-existing and Unix-only: `unused import: std::os::unix::fs::OpenOptionsExt` in `write_booru_auth`.

**Not yet verified on Windows or macOS.** None of the changes below touch `cfg` blocks, so they are platform-neutral, but a release build on all three targets is still required before shipping.

---

## 2. Fixed in this pass

### 2.1 YouTube: `Requested format is not available` — **fixed**

Root cause confirmed against [yt-dlp#16569](https://github.com/yt-dlp/yt-dlp/issues/16569), which reports the identical symptom. When yt-dlp finds YouTube account cookies it switches to the `web_creator` client, which requires a PO Token bound to the video ID. With no PO Token provider, *every* format is discarded and even the `/b` fallback fails. The reporter's own summary: *"If I remove the --cookies-from-browser it downloads OK."*

`start_row` passed `cookie_args()` to every download with no per-site decision, and the no-cookie retry only triggered on `is_cookie_error()` — which matches cookie *decryption* failures (DPAPI, locked database), not this error. So there was exactly one attempt, with cookies, and then failure.

Now: public content is attempted **without** cookies, and the session is attached on retry only when the error genuinely indicates authentication. Instagram, Weibo and the social networks still get cookies from the first request. The inverse fallback — cookies present but unreadable, retry without them — is preserved.

New functions: `needs_cookies_upfront()`, `needs_auth_error()`, `host_of()`, `host_matches()`.

### 2.2 Format selector without ffmpeg — **fixed**

`b/bv*+ba` had `bv*+ba` as its fallback: with no pre-merged format available it asked yt-dlp to merge with no merger present, aborting with `OSError [Errno 2]` — precisely what the comment above it said it wanted to avoid. Extracted into `format_selector()` and reduced to `"b"`. The ffmpeg branch and Bilibili's `-S res,fps,hdr,tbr` are untouched.

### 2.3 `--no-warnings` — **removed from `run_ytdlp`**

It silenced exactly the diagnostics that explain a failure: missing JS runtime, unsolved challenge, PO token limits, signature extraction. The application was receiving the explanation and discarding it. Still passed in `analyze_profile`, which parses JSON from stdout with `-J` and would be contaminated.

Consequence handled: with warnings visible, the last non-empty line is often a `WARNING:`, so `report_ytdlp_error()` now prefers an `ERROR:` line for the short label and keeps the full output of both attempts in `Ev::ErrorDetail`.

### 2.4 Profile downloads landing flat — **fixed**

`per_author` ("Create a subfolder per author") existed and defaulted to on, but `author` was only populated by the Profile tab and the browser capture. Pasting an Instagram profile into Downloads left it empty, so `dest_dir()` returned the root and everything piled up together.

`author_from_url()` now derives the profile name from the URL when the caller supplies none, covering LinkGrabber, paste, TXT/JSON import, clipboard and retry. It deliberately returns empty for non-profile paths (`/p/`, `/reel/`, `/stories/`, `watch?v=`, direct CDN links) rather than inventing folders called `watch` or `p`, and only accepts plausible usernames (alphanumeric, `.`, `_`, `-`, max 40 chars).

The derived author is used for the **folder only**, not the filename, so pasted links keep the names they always had.

### 2.5 Test coverage — **0 → 11 tests**

The repository had no tests at all. Added coverage for the logic that broke: host parsing and impostor-domain rejection, cookie policy, auth-vs-format error classification, format selection, author derivation, and filename sanitization. All pure logic — no network, no subprocesses, identical on all three platforms.

### 2.6 Documentation and licensing corrections

- `SECURITY.md` claimed *"HTTPS with rustls — no system OpenSSL"*. Inaccurate: `librqbit` enables `reqwest/default-tls`, Cargo features are additive and unified, and with both backends present reqwest defaults to **native-tls**. `Client::builder()` does not call `.use_rustls_tls()`. The resolved feature set confirms it: `[__rustls, __tls, default-tls, gzip, json, rustls-tls, ...]`. Corrected in `SECURITY.md` and `README.md`.
- `SECURITY.md` claimed *"No credentials or tokens are stored"*. Inaccurate: `booru_key` is part of `Settings`, which is serialized to eframe's plaintext store. Corrected and stated plainly.
- `LICENSE-HISTORY.md` listed cyberdrop-dl as MIT. The package the app actually installs is [cyberdrop-dl-patched](https://github.com/NTFSvolume/cdl), which is **GPL-3.0-only**. Corrected.
- `.gitignore` had `!/tips/LEEME.txt`, but the file is `README.txt` — the exception never matched, so the folder's documentation was silently excluded from the repository. Corrected.
- `tips/README.txt` claimed the GIFs are embedded into the binary by `build.rs`. They are not: `build.rs` only embeds the icon and version metadata, and `tips_dir()` reads them at runtime from next to the executable. Rewritten in English and corrected.
- `.github/FUNDING.yml` comments translated to English.
- Local build scripts were dropped from the repository: `cargo build --release` is the documented path, and CI runs `cargo test` before building anything.

---

## 3. Still open

### 3.1 P0 — Weibo wrapper URLs

`passport.weibo.com/visitor/visitor?...&url=<encoded>` is sent to a download engine verbatim and returns `Unsupported URL`. Three confirmed causes:

- `is_gallery_site()` (`main.rs:573`) matches by substring, and `passport.weibo.com` **contains** `weibo.com`, so the wrapper is routed to gallery-dl as-is.
- `normalize_profile_url()` has exactly one caller — the Profile tab's Analyze button — and only handles `/u/` paths. The other nine entry points into `add_url` never normalize.
- Deduplication compares raw URL strings, so the wrapper and the direct URL create two rows for the same video.

`host_of()` and `host_matches()` are now in place, which is the groundwork. What remains: a canonical normalization stage applied inside `add_url` before dedup and routing, safe unwrapping of the `url=` parameter with a host allowlist (rejecting `file:`, `javascript:`, localhost and third-party targets), and retry using the canonical URL.

**Blocked on:** which engine actually downloaded the verified 2560×1440 60 fps file. Unwrapping the URL and then routing it to the wrong engine fixes nothing.

### 3.2 P0/P1 — Progress reporting

The mechanism is in the code, not a mystery:

1. `main.rs` progress parser: `s.trim().parse::<f64>().unwrap_or(0.0)`. The template emits the literal `NA` when yt-dlp does not know the total, and `"NA".parse()` fails, so **unknown silently becomes zero**.
2. The progress bar then computes `downloaded / size` with `size == 0`.
3. The row only leaves `Resolving` for `Downloading` when `done > 0`, so a transfer with unknown byte counts looks frozen while the disk fills.

Fix: `Option<u64>` instead of `u64`, and an indeterminate bar when it is `None`. There is also no phase state for merging or post-processing, so a Bilibili or 1080p YouTube merge shows the last download percentage frozen.

### 3.3 P1 — Remaining items

| Item | Location | Note |
|---|---|---|
| JS runtime (Deno) | `run_ytdlp`, `spawn_*_check` | yt-dlp now requires an external JS runtime for full YouTube support ([#15012](https://github.com/yt-dlp/yt-dlp/issues/15012)). Per the [EJS guide](https://github.com/yt-dlp/yt-dlp/wiki/EJS), dropping `deno.exe` next to `yt-dlp.exe` is enough on Windows — no flags. The official PyInstaller build already bundles the EJS scripts, so step 2 of that guide does not apply. Should be opt-in |
| Booru key at rest | `Settings`, `main.rs` save | Plaintext in the settings store. DPAPI on Windows, with macOS/Linux equivalents — same threat model as the planned Telegram session |
| gallery-dl file counter | `galdl_exec` | Counts every non-empty stdout line as a file. Verify what gallery-dl actually writes to stdout before building progress on it |
| Substring host checks | `hosters.rs`, `CYBERDROP_SITES`, `referer_for` | `host_matches()` exists now; the remaining call sites should adopt it. `referer_for` currently sends a weibo.com Referer to anything containing "weibo" |
| GIF panel | `load_random_tip_gif` | Four silent `return Vec::new()`; a corrupt GIF is indistinguishable from none. Frame cap of 120 but no pixel cap: 1920×1080 × 120 ≈ 950 MB of texture |
| `short_hash` | `main.rs` | Uses `DefaultHasher`, whose algorithm is not guaranteed stable across Rust versions, while the comment calls it stable. Changing it renames already-downloaded content, so defer |
| `cargo fmt` | all files | 217 hunks. Should be one commit containing nothing else |
| Receiver concurrency | `receiver.rs` | Single-threaded accept loop; a connection that opens and sends nothing blocks it for the 10 s read timeout. Localhost only |
| Code signing | CI | Binaries are unsigned; SmartScreen and AV warn. Needs a signing strategy and secret handling |
| Modularization | `main.rs` | 5 900+ lines carrying UI, routing, process control, installers and progress parsing. Only after the P0 work is covered by tests |

### 3.4 P2 — Future features

- Telegram batch downloader — architecture proposal pending, deferred past v1.6.2.
- MEGA public links — **implemented and verified end-to-end against the live service**: single 140.7 MB file (MAC validated) and a 107-file public folder. See `MEGA-IMPLEMENTATION-DECISION.md`.
- MEGA account login (Phase 2) — deliberately out of scope; see `MEGA-FEASIBILITY.md`.

---

## 4. Regression areas

These work today and are not rewrite targets. Verify before any release:

Bilibili (quality sorting, audio/video merge), Booru (search, pagination, credentials), TikTok (video, profile, carousels, capture import), Douyin (capture), Instagram (cookies, archive resume), torrents (magnet, `.torrent`, pause/resume, instance forwarding), native HTTP with resume, Pixeldrain / GoFile / MediaFire, LinkGrabber, themes and backgrounds, EN/ES localization, Linux and macOS builds.

---

## v1.6.2 — what was added and what was learned

**Shipped**

- Native V2PH extractor (`src/v2ph.rs`): albums, model, agency, category and
  country pages, with the preview grid and the native HTTP engine.
- Native Firefox cookie reading (`src/cookies.rs`), new `rusqlite` dependency
  compiled with `bundled`.
- In-app sign-in able to parse any HTML login form, with verification against
  the site rather than trusting a 200.
- A User-Agent setting and a one-click detector that reads the value from the
  browser's own request to the local receiver.
- A V2PH browser-capture script.
- Background page-chaining for gallery listings, and a Clear list button.

**Corrected during the work, and worth keeping visible**

- ADR-001 claimed V2PH "has no bot protection", drawn from four clean probes.
  Wrong in the part that mattered: `/login` is behind Cloudflare's challenge and
  the site rate-limits bursts with `403` elsewhere.
- ADR-002 recommended an in-app sign-in as the answer for V2PH. It was built,
  then measured not to work there. Marked superseded, with the evidence.
- A `403` that persisted for a while was diagnosed as a possible TLS-fingerprint
  ban. It later lifted on its own, so that was an overreach — it was rate
  limiting.
- The App-Bound Encryption warning was stated as a property of Chromium; it is
  a property of Windows.

**Still open**

- **MEGA pause/resume mid-transfer has never been exercised against a real
  link.** It is the only engine path in the project with no live verification.
- `cargo clippy` and the unit tests run on Linux and in CI; macOS builds are
  produced by CI but have not been run by hand.
- Telegram batch downloader: still an architecture proposal, deferred.
