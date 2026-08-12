# Todo Downloader v1.6.7

**Single-post capture for Douyin and TikTok, and the fixes found while building it**

Builds on v1.6.5. See `RELEASE-NOTES-v1.6.5.md` for the ffmpeg merge fix and the
bitrate setting. Notes for earlier versions are published on the
[Releases page](https://github.com/AcidClawX41/todo-downloader/releases); only
the current and previous ones are kept in the repository.

---

## Capture a single post, not a whole profile

Grabbing the photos of one post used to mean capturing the entire profile and
sifting through it. Now there is a button on the post itself.

**Capture → Capture a single post** offers three ways to install it:

- **Copy userscript** — paste into Tampermonkey or Violentmonkey
- **Save .user.js** — drag the file onto a browser tab and the manager offers
  to install it. Two clicks, no copying
- **Copy bookmarklet** — nothing to install at all

From then on, a **⬇ Capture this post** button appears on every Douyin and
TikTok publication. One click and the photos land in the app.

### Why the browser has to do it

Douyin signs its API with X-Bogus and msToken, so requesting it from outside
returns nothing. The profile capturer works around this by intercepting
responses while the page scrolls — but for a post that is *already open* that
response has already gone past. So the script reads what the page has in front
of it.

### How the carousel is found

The first three attempts guessed at the DOM and all three failed. A post with
five photos returned one. What settled it was dumping the actual DOM instead of
reasoning about it, and the structure turned out to be simple:

Every slide is painted twice — once small and centred, which is what you see,
and once at full width behind it, blurred, as the backdrop. The viewer is a
vertical feed and each post occupies a band:

```
2100x415 @ (0, -415)    previous post
2100x415 @ (0, 0)       THIS post, slide 1
2100x415 @ (2100, 0)    THIS post, slide 2
2100x415 @ (0, +415)    next post
```

**Slides of one post share `top` and differ in `left`; different posts differ in
`top`.** That identifies the whole post without depending on class names — which
Douyin obfuscates and changes on every deploy — without clicking arrows, and
without waiting for anything to load. It is all already on the page.

The earlier approach, stepping the carousel by clicking, never worked: the
button could not be found, and even if it had been, it would have been a detour
to reach something already present.

### Original quality, and where it comes from

Douyin serves the `~noop` variant — the unprocessed original, no watermark, no
rescaling — directly in the DOM. It is taken as-is. For anything that does not
carry one, `quality_variants()` in the application already tries `~noop` first
and works down. That logic is not duplicated in JavaScript: two places to be
wrong is one too many.

### Videos

A video post exposes only a `blob:` URL — a Media Source Extensions reference
that exists solely inside that tab. It is not sent, because it would fail. The
post URL goes to the application instead and yt-dlp resolves it, at the quality
your bitrate setting asks for.

This matters more than it sounds: the page *does* contain playable URLs — the
background music (`ies-music/….mp3`) and videos belonging to other posts in the
feed. Grabbing those would have downloaded anything except what was asked for.

The blurred backdrop is the video's poster, so a video appears in the selection
grid with a thumbnail, exactly like a photo.

### Where the files land

A new checkbox in **Settings → Downloads**: *Single posts go to the selection
grid*, on by default. Turned on, a captured post opens in Profile with
thumbnails and checkboxes. Turned off, it goes straight into the queue.

The script sends only *what* it captured, never *where* it should go, so
changing your mind does not mean reinstalling anything in the browser.

### On Chrome and Vivaldi

Those browsers block a page from reaching `127.0.0.1` (Private Network Access).
The userscript uses `GM_xmlhttpRequest`, which runs in the extension's context
and is not subject to that, so it works. A bookmarklet runs *inside* the page
and is, so there it falls back to saving a JSON you import. Firefox delivers
directly either way.

---

## Fixes

**Douyin video downloads failed every time.** yt-dlp's Douyin extractor answers
`Fresh cookies (not necessarily logged in) are needed` without a session, and
two separate things were wrong: Douyin was missing from the list of sites that
get cookies on the first attempt — even though the Profile tab has always said
*"required for Douyin"* — and that message was not recognised as an
authentication failure, so the retry-with-cookies path never fired either. The
download died without ever using cookies that were already configured.

**Chinese, Japanese and Korean titles showed as `□□□` on Arch-based systems.**
The CJK font was looked up at two fixed paths, both of them Debian's. Arch — and
therefore Manjaro and Garuda — keeps Noto CJK somewhere else entirely, so
nothing was found and the function returned in silence. It now scans the
directories each distribution actually uses, matching *font* names rather than
package paths, which covers Arch, Debian, Fedora and openSUSE at once.

**Long warnings wrapped one word per line.** An `egui::Area` inherits no width,
so without an explicit maximum the text was squeezed to the narrowest possible
column.

**A console window flashed at startup on Windows.** Language detection shells
out to PowerShell, and the application has no console of its own, so Windows
handed the child process a brand new window. Only visible on a first run, which
is the only time the language is detected.

**Engine detection could hang forever.** `Command::output()` waits without
limit. A Python launcher orphaned by an uninstalled interpreter — a
`cyberdrop-dl.exe` whose `python.exe` no longer exists — leaves the process
stuck, and with it the detection thread. The application then never learns
whether that engine is available: not present, not absent, simply no answer.
Detections now run with a five-second limit and the child is killed and reaped
if it exceeds it.

**Resolutions are shown for captured items.** The preview already downloaded and
decoded the full image before shrinking it to 320 px; those dimensions were
being discarded. They are not filled in for videos, where what was decoded is
the poster, not the video — showing `360×640` next to a 1080p file would be a
lie.

**Cookies explain themselves.** The `no cookies` line in the sidebar now says on
hover what stops working without a session. And when a download fails with
something that looks like an authentication error *while no cookies are being
sent*, an amber banner appears above the queue pointing at the cause. Only then:
a permanent warning is a warning nobody reads.

---

## Console scripts follow the interface language

Messages produced by the browser scripts — the panel, the counters, the errors —
now come out in whichever language the application is set to. They are resolved
when the script is generated, because the script cannot ask the application
anything: it may well be running with the receiver switched off.

Comments *inside* the emitted script are short and in English on purpose. It is
a file you paste into a browser, not code anyone maintains from there; the
reasoning lives in the Rust source, which is where it gets read.

---

## A Windows installer

The portable executable remains the primary download and always will: it needs
nothing, writes nothing outside its own folder and runs from a USB stick. But
"download an .exe and put it somewhere" is not how most people expect to
install a program, so a Setup is now published alongside it.

- **No administrator rights.** It installs into your user profile, so Windows
  shows no UAC prompt. That matters here: the binaries are not code-signed, and
  a UAC dialog reading *"Unknown publisher"* in red is a good way to scare
  people off something harmless. Anyone who prefers a machine-wide install
  under Program Files can choose it at the start, and only then is elevation
  requested.
- **Terms of use, shown for acceptance**, in English or Spanish depending on
  the installer language, and installed alongside the program so they can be
  read again later. They cover the absence of warranty, the limitation of
  liability, and — plainly — that the person operating the program decides what
  it downloads and is responsible for that decision.
- **The GPL is not the acceptance screen.** Section 9 of the licence says you
  are not required to accept it in order to receive or run a copy, only to
  redistribute or modify. Gating the program behind an "I accept" on the GPL
  would assert a condition the licence itself denies, so the GPL ships as
  `LICENSE` and the terms point at it.
- **It bundles no helper programs.** yt-dlp, gallery-dl and ffmpeg are still
  downloaded from Settings when you ask for them. A copy frozen inside an
  installer would stop working on YouTube within weeks.
- **It does not register the `magnet:` protocol.** That modifies a system-wide
  association, and it stays where it was: a deliberate choice in Settings.
- **Uninstalling leaves your downloads and settings alone.**

An unsigned installer triggers the same SmartScreen warning as an unsigned
executable. This adds a normal installation experience; it does not add
reputation.

---

## Known limitations

- **A video post is one file, not several.** If Douyin ever allows carousels
  mixing photos and video, only the video would be picked up.
- **Single-post capture needs the post open in a browser.** There is no way to
  paste a Douyin post URL into the application and have it resolved: the API is
  signed, and reimplementing that signature is exactly the kind of thing that
  breaks every few weeks.
- Everything listed in `RELEASE-NOTES-v1.6.5.md` still stands, including that
  **MEGA pause/resume mid-transfer remains untested** against real links.

## Licence

GPL-3.0-or-later. v1.4.0 and earlier remain available under MIT — see
`LICENSE-HISTORY.md`.
