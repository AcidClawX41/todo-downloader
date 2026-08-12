# Todo Downloader v1.6.5

**Hotfix: audio and video were not being merged outside Windows**

A patch on top of v1.6.2. Notes for that release and for v1.6.0 — the MEGA.nz
engine and the preview grids — are published on the
[Releases page](https://github.com/AcidClawX41/todo-downloader/releases).

---

## YouTube downloads came out unmerged on Linux and macOS

Reported after v1.6.2: on Windows, YouTube videos downloaded with picture and
sound in one file, as expected. On Linux the two streams arrived separately and
were never joined.

The cause was a single line, and the platform split explains itself once you see
it.

Detection returns the **full path** to ffmpeg when the application installed it
itself — which only happens on Windows — and the bare word `ffmpeg` when it
found one in the system `PATH`, which is the normal case on Linux and macOS.
That value was then handed to yt-dlp like this:

```rust
if let Some(dir) = Path::new(ff).parent() {
    base.push("--ffmpeg-location".into());
    base.push(dir.to_string_lossy().into_owned());
}
```

**`Path::new("ffmpeg").parent()` does not return `None`. It returns an empty
path.** So the application was passing `--ffmpeg-location ""`, yt-dlp looked for
the binary in a directory that does not exist, found nothing, and could not
merge. Windows never hit it, because there the path was always a real one.

`--ffmpeg-location` is now only passed when there is an actual directory to
point at. Without it, yt-dlp finds ffmpeg on the `PATH` by itself, which is
exactly the right behaviour when the binary came from the system.

Two unit tests pin this down, including the empty-parent case that caused it.

## Galleries now load the whole profile

The background chain fetched four pages behind the first one and then stopped,
leaving *Load more* to be pressed repeatedly.

It now keeps going until the profile ends. The chain already stopped on its own
when a page came back empty or a request failed, so no new stopping condition
was needed — only the artificial four-page limit was removed.

A cap of 40 pages remains as a **safety net**, not a design limit: it exists so
that an extractor which never stops returning results cannot keep requesting
pages forever. That is 1200 items, more than almost any profile holds, and
*Load more* re-arms the chain if it is ever reached.

**The button stays.** Nothing about the manual path changed.

V2PH is deliberately excluded from the chain, as in v1.6.2: there each "page" is
an entire album rather than a single request, and chaining trips the site's rate
limiting.

## Images and videos are counted separately

The gallery header now shows how many of each the profile holds — `🖼 214  🎬 31`
— counted over the **whole** list rather than the filtered view. Filtering to
videos while still seeing how many images are behind the filter is the point.

## A quality setting that was quietly costing bitrate

Fixing the merge bug raised a fair question: were YouTube downloads actually
coming out at full quality? Resolution and frame rate were. Bitrate was not, and
the reason is in yt-dlp's own default ordering:

```
res, fps, hdr:12, vcodec, channels, acodec, size, br, …
vcodec order: av01 > vp9 > h265 > h264 > …
```

Resolution and fps decide first, so the largest, smoothest picture was always
selected. But **codec outranks bitrate**, and the default codec preference puts
AV1 first — the most efficient encode, which is also the one with the fewest
bits. At 1080p that is roughly 1500–2000 kbps of AV1 where the H.264 encode of
the same video carries over 4000.

Neither choice is wrong. AV1 at a lower bitrate looks comparable and the file is
a fraction of the size; H.264 at a higher bitrate is a safer archive and plays
in anything, including editors that still choke on AV1. So this is a **checkbox
in Settings → Downloads**, on by default: *Prefer bitrate over codec efficiency*.

Turned on, `-S res,fps,hdr,tbr` is passed. Resolution and fps stay first, so
nothing is traded away — the ordering only changes which of several encodes of
the *same* picture wins. Turned off, yt-dlp's factory behaviour applies.

Bilibili is unaffected either way: it has always forced this ordering and still
does, because it publishes each resolution in both AVC and HEVC and the AVC
carries considerably more bitrate.

One limit worth stating plainly: YouTube's enhanced-bitrate 1080p — the Premium
tier — requires an account **and** a PO token. Nothing here reaches it.

## Messages now follow the language you selected

The interface has been translated since v1.0, but a whole class of messages was
not: the ones written by the download engines themselves. V2PH's session
instructions, the MEGA errors, the Firefox cookie traces, the helper-install
failures and the gallery-dl diagnostics were all hardcoded in Spanish and stayed
that way with the application set to English.

They were missed for a structural reason rather than an oversight. Interface
labels are drawn by a function that already receives the selected language;
these messages are produced inside asynchronous tasks that have no interface
state to receive it from. The fix is a single global holding the current
language, set at startup and updated when you change it in Settings. The
application has one language at a time, so a global is a faithful description of
reality rather than a shortcut around one.

Converted this release: the V2PH session, quota and rate-limit notices, the
sign-in errors, the Cloudflare notice, the Weibo `403` hint, the MEGA error
list, the Firefox cookie diagnostics, the file-host and torrent errors, and the
engine-installation messages.

## Licence

GPL-3.0-or-later. v1.4.0 and earlier remain available under MIT — see
`LICENSE-HISTORY.md`.
