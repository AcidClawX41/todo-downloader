# Todo Downloader v1.6.0

**Native MEGA.nz public-link downloads, and a fix for YouTube**

---

## MEGA.nz, natively

Public MEGA file and folder links now download directly from the queue. No MEGAcmd, no browser automation, no helper executable — the engine is compiled into the binary.

MEGA stores ciphertext it cannot read: your decryption key lives only in the URL fragment and is never transmitted to any server. This implementation preserves that. The key is parsed locally, used locally, and **never sent to MEGA's API**.

- **Resumable.** AES-CTR is seekable, so a `.part` file resumes at any byte offset — including offsets that are not on a 16-byte boundary, which is where naive implementations silently corrupt data.
- **Verified.** CTR provides confidentiality but not authenticity. Every download is checked against MEGA's own chunked file MAC **before** the final filename is created. A corrupt, truncated or wrong-key transfer never appears as a completed file; the partial data is set aside as `.part.corrupt`.
- **Folders expand.** A public folder link becomes one queue row per file, each with its own progress, pause and error state.
- **Honest progress.** The exact size comes from the metadata, so there is no invented 0%. Integrity verification is its own visible phase rather than an apparent freeze.

**Account login is not supported.** No MEGA credentials are requested, and none are stored. Public links need neither.

One new dependency: `aes` (MIT OR Apache-2.0), a pure-Rust block cipher. No SQLite, no C toolchain, no registry access, no device fingerprinting.

## YouTube downloads work again

Downloads were failing with `Requested format is not available` on ordinary public videos.

The cause was cookies. When yt-dlp finds YouTube account cookies it switches to a client that requires a PO Token; without a PO Token provider **every** format is discarded and even the fallback selector fails. The application was sending cookies to every download regardless of site, and the no-cookie retry only triggered on cookie *decryption* failures — so this error produced exactly one attempt and then gave up.

Cookies are now sent **only where they are needed**. Public content is tried anonymously first, and your session is attached on retry only if the site actually asks for authentication. Instagram, Weibo and the social networks still get cookies from the first request, because they list nothing without one.

Two related fixes:

- Without ffmpeg, the format selector no longer falls back to separate video and audio streams — it asked yt-dlp to merge with no merger available, which aborted.
- `--no-warnings` has been removed. It was silencing exactly the diagnostics that explain a failure. Error details now keep the full output of both attempts, with an `ERROR:` line preferred for the short label.

## Pick what you download: preview grids

Profile analysis used to hand you a list of titles. You had to guess.

Every browsable profile now renders as a **thumbnail grid** — the actual photos and video covers, click to select.

- **Instagram and Weibo** are listed with `gallery-dl -j --no-download`, which dumps metadata without fetching anything. Each tile carries resolution, format, date, and position within the post (`3/10`), so a carousel is visible as a carousel.
- **TikTok and Bilibili** profile analysis shows the cover yt-dlp already returns for each entry.
- Filter by images or videos, page through with *Load more*, and queue only the selection.

Thumbnails are fetched once per item through a shared gate, decoded off the async pool, and the ones that fail (expired CDN link, anti-hotlink) fall back to a media icon instead of spinning forever.

### Weibo profiles are browsed through the photo wall

gallery-dl's Weibo user extractor only dispatches to `?tabtype=feed`, which hits `/ajax/statuses/mymblog`. Two problems with that endpoint: it answers **403 without a session**, and the `pic_infos` it returns carries **already-downscaled variants** — the grid showed 810×1080 while the same post pasted by hand downloaded at full size.

`?tabtype=album` goes through `/ajax/profile/getImageWall` and then re-fetches each post with `/ajax/statuses/show` — the exact call the single-post extractor makes. Same response, same resolution. Weibo profiles now use it by default, falling back to the feed only if the album comes back empty.

It also lists only posts that contain media, so text-only posts never reach the grid.

## Non-Latin-1 titles no longer kill downloads

A TikTok video failed with `[Errno 22] Invalid argument` while the eight next to it in the same profile downloaded fine. The video was not the problem — its title was.

On Windows the Python-based helpers inherit the system code page (cp1252 on a Spanish install). When yt-dlp writes a title containing a character that page cannot represent — an emoji, a kanji, a typographic quote — the stdout wrapper raises and the interpreter aborts. Every helper process is now launched with `PYTHONIOENCODING=utf-8` and `PYTHONUTF8=1`.

## Booru searches no longer hang

`gallery-dl` was run with `cmd.output()` and no deadline, so a site that accepted the connection and then stalled left the spinner turning forever with no way out. Listing calls now run under an explicit timeout and the process tree is killed when it expires.

Switching search or page mid-flight also used to let a slow earlier request overwrite the newer results. Every search now carries a generation counter; replies from a superseded search are discarded.

## Profile downloads are grouped by author

"Create a subfolder per author" existed and was on by default, but the author was only filled in by the Profile tab and the browser capture. Pasting an Instagram profile into Downloads left it empty, so everything landed loose in the root of the download folder.

The profile name is now derived from the URL when the caller supplies none — covering LinkGrabber, paste, TXT/JSON import, clipboard and retry. It deliberately creates no folder for non-profile paths (`/p/`, `/reel/`, `watch?v=`, direct CDN links) rather than producing folders called `watch` or `p`.

## Under the hood

- **57 unit tests**, up from zero. They cover URL parsing and impostor-domain rejection, cookie policy, format selection, MEGA key unpacking, CTR resume equivalence across non-aligned offsets, MAC corruption detection, and path-traversal safety.
- CI runs the tests before building anything: a failing test blocks the binaries.
- The macOS Intel release job moved to a native Intel runner. It was cross-compiling from Apple Silicon, which cannot work here: `librqbit` pulls in native-tls, and the Homebrew OpenSSL on an arm64 runner is arm64-only.

## Documentation and licensing corrections

- `SECURITY.md` claimed all traffic used rustls with no system OpenSSL. That was inaccurate: `librqbit` pulls in `reqwest/default-tls`, Cargo features are additive, and reqwest defaults to native-tls when both are present. Corrected.
- `SECURITY.md` claimed no credentials are stored. Booru API keys are saved in the settings file in plaintext. Now stated plainly.
- `LICENSE-HISTORY.md` listed cyberdrop-dl as MIT. The package actually installed is `cyberdrop-dl-patched`, which is GPL-3.0-only. Corrected.
- `.gitignore` excepted `tips/LEEME.txt`, but the file is `README.txt` — the folder's documentation was silently excluded from the repository.
- `tips/README.txt` claimed `build.rs` embeds the GIFs into the binary. It does not; they are read at runtime from beside the executable.

## Verification

The MEGA engine was tested against the live service.

**Single file:** a 140.7 MB public link downloaded, passed integrity
verification and completed. The archive's internal structure and per-file
checksums read back correctly, confirming the decryption is byte-exact.

**Public folder:** a 107-file folder expanded into individual queue rows and
downloaded. Getting there took three real bugs that only a live run could
surface:

- The transfer URL was rejected for not being HTTPS. MEGA serves storage URLs
  over plain HTTP by default, since the payload is already end-to-end
  encrypted. Fixed by requesting `ssl:2` and upgrading the scheme as a
  fallback.
- Every file re-listed the whole folder to obtain its node key — 107 identical
  API calls in seconds, which MEGA rate-limits, failing the entire batch. The
  listing is now cached per folder.
- A queued file arrives as `/folder/H#K/file/NODE`, which also parses as a
  folder. It was being treated as a folder to expand, so each row re-expanded
  itself in an endless loop that never downloaded anything. Expansion now
  requires the link to have no node, and there is a regression test for it.

Pause/resume mid-transfer and transfer-URL expiry refresh are implemented but
have not yet been exercised against real links. Treat them as untested.

## Known limitations

- MEGA **folder downloads queue every file**; there is no per-file selection view yet.
- Preview grids cover Instagram, Weibo, TikTok and Bilibili. Pinterest, Tumblr, DeviantArt and the rest are still downloaded whole.
- Weibo and Instagram both need a session in the browser selected in Settings, or a `cookies.txt`.
- Large folders are slower per file than a single large download: each file
  still costs one metadata request, and requests are deliberately spaced out.
  Raising the concurrency setting helps.
- MEGA account login, uploads and cloud-drive browsing are out of scope.
- Binaries remain unsigned; SmartScreen may warn.

## Licence

GPL-3.0-or-later. v1.4.0 and earlier remain available under MIT — see `LICENSE-HISTORY.md`.
