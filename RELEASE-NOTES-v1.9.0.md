# Todo Downloader v1.9.0

**Kemono and Coomer, Patreon without Cloudflare in the way, and Chromium
cookies on Linux and macOS**

Builds on v1.8.8. See `RELEASE-NOTES-v1.8.5.md` for Hugging Face models and
multi-connection downloads; notes for earlier versions are published on the
[Releases page](https://github.com/AcidClawX41/todo-downloader/releases).

---

## Kemono and Coomer

A new **Kemono** tab, and native support in **Profile** for `kemono.cr` and
`coomer.st`. The old `.su` and `.party` addresses are recognised and rewritten
to the current domain, so a link saved a year ago still works.

**Search creators by name.** Both sites publish their complete creator list
through the API — about 108,000 entries and 3.4 MB on Kemono, 5.9 MB on
Coomer, measured on 21 October 2026. It is fetched once per session, parsed
off the interface thread, and searched in memory: by name or numeric id,
filtered by service (Patreon, Fanbox, Fantia, Gumroad, Boosty, SubscribeStar,
DLsite, Afdian, Discord on Kemono; OnlyFans, Fansly, CandFans on Coomer), with
exact prefixes first and then by how many users favourite each creator. One
click opens the creator in the Profile grid.

**A creator or a single post pasted into Profile** is listed through the
site's own API — no account, no cookies, no gallery-dl — with the site's
thumbnails (`img.kemono.cr/thumbnail/…`, 45 KB where the original is several
megabytes). Posts arrive 50 at a time, newest first, which is exactly one API
call; *Load more* fetches the next 50. A banner above the grid says who the
creator is, how many posts they have and which batch you are looking at. The
URL's own `?q=` and `?tag=` filters are passed to the API as they are, and the
Kemono tab has two fields that write them for you.

Why not gallery-dl for the grid: its extractor walks the creator from start to
end in one pass, which for a 542-post illustrator with forty images per post
is twenty thousand tiles at once; every run unpacks a Python interpreter; and
its listing carries no thumbnails. gallery-dl is still what downloads a
**whole creator pasted into Downloads**, in one pass, and what handles Discord
servers.

Two things worth knowing, both measured against the site:

- The API answers `403` to anything that does not send `Accept: text/css`,
  with a message saying so. That is the site's way of telling a client that
  read the documentation from a browser that stumbled in; gallery-dl sends the
  same header, and so does this.
- File links are content hashes (`/data/c8/90/c890…be.png`). They redirect to
  whichever file server is on duty and **never expire**, so the grid does not
  show the "download soon" warning it shows for Instagram or X. A file link
  pasted on its own goes to the native HTTP engine with resume, not to
  gallery-dl.

Images embedded in a post's text (`<img src="/…">`) are picked up when that
single post is opened; the listing endpoint only carries a snippet of the text.

## Patreon: creators resolved through the API

Patreon's web pages now sit behind a Cloudflare challenge — `403 Just a
moment…` — for anything that is not a browser, and Python's TLS fingerprint
is not a browser. That page is where gallery-dl looked up a creator's campaign
id, so creator listings died with *Unable to extract campaign ID* whether or
not a session was attached.

The API is not behind the challenge. `GET /api/campaigns?filter[vanity]=`
returns the campaign with its id, without a session, case-insensitively, adult
campaigns included (checked against one marked `is_nsfw`). So:

- **A creator is listed natively**, through the same streaming path that
  already served collections: one request per hundred posts, locked posts
  skipped, the creator's name as author.
- **A whole creator pasted into Downloads** is handed to gallery-dl as
  `patreon.com/id:<campaign>`, a form its extractor accepts without fetching
  the page. The URL's filters (`?filters[tag]=`, `?sort=`) are preserved.
- **An expired CDN link of a Patreon post is refreshed through the API**, not
  through the page. The retry used to re-resolve the post with gallery-dl,
  which fetches the page, which is the challenge. The file is matched by its
  post and media hash, never by its name — `1.png` repeats across posts.

A creator who closed their page returns an empty list from the API. That is
the truth, and it is reported as such rather than as a failure.

## Patreon: your session, from the browser

The Patreon listers (a post, a collection, now a creator) make their own
requests and only knew how to read a `cookies.txt`. With *Use browser cookies*
selected and no file, they ran **without a session**, and a paid post you
already had access to came back as `current_user_can_view: false` — reported
as "locked by tier" with the session one setting away. They now read the
session the same way V2PH does: from the file if there is one, else from
Firefox's database directly, else as below.

## Chromium cookies on Linux and macOS

Firefox stores cookies unencrypted; Chromium encrypts each value with a key
kept in GNOME Keyring or KWallet on Linux and in the Keychain on macOS.
gallery-dl and yt-dlp already decrypt those (both ship a `cookies.py`, one
ported from the other), so the native engines now ask them rather than
reimplementing three key stores and D-Bus in Rust.

gallery-dl is run with `--cookies-from-browser chrome/.patreon.com` — the
browser specification **limited to one domain** — and `--cookies-export`,
against a direct-link URL with `--no-download`: nothing is fetched, the
cookies are loaded and written. If that yields nothing, yt-dlp with
`--cookies-from-browser --cookies FILE --simulate` does the same without the
domain filter, and the result is filtered here. The file lives for the length
of one read and is deleted; what is kept is the `Cookie` header, in memory,
for half an hour, so that on macOS the Keychain prompt — which `security`
raises once per process — does not appear on every page. Choose *Always allow*
when it does.

This applies to the Patreon listers and to V2PH. On Windows, Chrome 127+
encrypts cookies with App-Bound Encryption and no external tool can read them;
the attempt is made anyway, costs one process, and the `cookies.txt` route
remains the one that works there.

It could not be tried end to end where this was written — there is no browser
and no keyring in a build container. The two gallery-dl options were read in
its source (`option.py`, and `job.py:handle_finalize`, which calls
`cookies_store`), and the yt-dlp form is its documented way to export a jar.
When it fails, the lister runs without a session, as it always did, never with
a new error.

## Instagram: the error that means "update gallery-dl"

Instagram changes its API every few weeks and gallery-dl keeps up: six
Instagram fixes between 12 September and 17 October 2026 (`posts`, `reels`,
`doc_id`, `user`, `highlights`, `stories`). A binary from two months ago fails
with a `KeyError`, a `TypeError` or a GraphQL `400` that has nothing to do
with cookies or the network.

- That class of error now carries the right advice: *Settings → Engines →
  Update to latest*, before anyone is sent to check their session.
- *Settings → Engines* shows **how many days old** the bundled gallery-dl is,
  in amber past three weeks (more than two gallery-dl releases).

## Hotfix: Kemono and Coomer downloads, and Instagram's silent empty list

Found in use within the hour of release, and corrected twice — the first
diagnosis was wrong, and the record keeps both.

### Kemono and Coomer files that never arrived

Forty files queued from a Kemono post stayed at 0 % with no size; after a
first fix they failed with `error sending request for url
(https://n1.kemono.cr/data/…)`.

**The first diagnosis was wrong.** `kemono.cr/data/<hash>` answers `302` to
a file server and sets DDoS-Guard cookies in that reply, and the native
engine followed the redirect without them. That was fixed — the redirect is
now resolved by hand with the cookies attached, as a browser does — but it
was not the cause.

**The cause is Kemono's, not ours.** Since June 2026 the original-file
servers of both sites (`n1`–`n4.kemono.cr`, `n3.coomer.st`) do not deliver:
connection reset, or `503 upstream connect error … connection timeout`.
gallery-dl documents it ([mikf/gallery-dl#9442](https://github.com/mikf/gallery-dl/issues/9442):
"it's currently impossible to download the actual files"), and since 1.32.3
it downloads the **thumbnail** by default instead. Measured on 24 October
2026: originals still `503`, `img.kemono.cr/thumbnail/data/…` answers `200`
with a JPEG of 533×800 (449×800 on Coomer). The `.jpg` files gallery-dl left
in the download folder were exactly those thumbnails.

What the application does now:

- **Tries the original twice**, not three times, with the redirect and
  cookies resolved by hand. If the original ever comes back, it downloads.
- **Remembers the outage for ten minutes per site.** The first failure arms a
  breaker, and the rest of the queue skips straight to the next step instead
  of each file waiting out its own timeout. Measured: 21 s for the first file,
  0.2 s for the second.
- **Saves the preview** — `<name>_preview.jpg`, so a 533-pixel JPEG can never
  be mistaken for the original — when *Settings → Kemono / Coomer* says so (on
  by default). The queue row is renamed to match, and a notice says why, once
  per session. Videos and archives have no preview and fail with an
  explanation.
- **Says it before you queue.** After listing a creator or a post, the grid
  probes one file with a single byte; if the originals do not answer, the
  banner says so and the breaker is already armed.
- **Never falls back to the post page.** The generic retry used to re-resolve
  a failed file through its page with gallery-dl, and for Kemono the page is
  the whole post: forty queued files became forty downloads of the complete
  post, in parallel, under other names.
- A `429` now waits 8 s × attempt; names no longer end in `.png.png`; and a
  native download waiting for the server reads «connecting…», not «yt-dlp».
- The shared HTTP client now has a **read timeout** of 60 s, reset on every
  chunk received. A server that accepted the connection and then sent nothing
  used to leave a row at 0 % forever, with no error and no retry.

### Instagram: an empty list with no error means "update gallery-dl"

`gallery-dl -j … instagram.com/<user>/posts/` returned `[]` with exit code 0
and an empty stderr, and the application said "usually a missing session".
That is the exact symptom gallery-dl fixed in **1.32.17** (17 October 2026):
"ensure non-zero exit status on GraphQL error", together with new profile and
posts queries.

- *Settings → Engines* now shows the gallery-dl **version** in use and the
  latest one published, in amber when it is behind. The latest version is
  asked of GitHub only when Settings is opened or Instagram comes back empty —
  never at start-up — with a fallback that reads the redirect of
  `github.com/…/releases/latest`, which has no rate limit.
- An empty Instagram listing explains the version cause first, with both
  numbers, and only sends you to check your session when gallery-dl is
  current. The *Update to latest* button installs today's build
  (`1.33.0-dev`), which includes the fix.

### The Chromium cookie bridge did not work for Patreon

Testing the bridge's arguments with the real tools exposed a bug: the dummy
URL handed to gallery-dl, `https://patreon.com/todo-downloader-sesion.png`, was
claimed by gallery-dl's **Patreon** extractor, which tried to open it as a
creator page, hit the Cloudflare challenge and exited without exporting
anything (`ChallengeError … exit 8`). It is now a reserved `.invalid` domain
that no extractor claims and that never resolves: gallery-dl exits 0 and
writes the file; yt-dlp fails to resolve, exits 1, and writes it anyway. Both
verified. The temporary folder is private (`0700`) on Linux and macOS, since
yt-dlp dumps the whole jar for an instant before it is filtered.

### Instagram and X: listings that stopped, videos that could not download

Reviewed against gallery-dl's current extractors and a real X listing
(1722 lines from a public profile).

- **Listings were cut at five minutes, silently.** The limit was total, from
  start-up, even while gallery-dl was delivering results; a large Instagram
  profile (6–12 s between requests) or X profile was cut halfway, and because
  the cut was reported like a user «Stop», nothing was said and the «fetching
  more» spinner kept turning forever. It is now an *inactivity* limit, counted
  from the last line on stdout or stderr. A timeout is reported, and what was
  already listed stays in the grid.
- **Rate-limit waits are shown, not mistaken for a hang.** When X or Instagram
  limit requests, gallery-dl announces «Waiting for 15 minutes until 13:45:12»
  and sleeps. The application now shows that wait above the grid, allows it
  (up to 20 minutes) instead of killing the listing, and removes the notice
  when results resume.
- **A list cut short by an error says so.** If gallery-dl ends with an error
  after delivering part of a profile — X's «Unable to retrieve Tweets»,
  Instagram's 401 — the banner says the list may be incomplete, with the last
  message and the matching advice. Before, half a profile looked like all of
  it.
- **Instagram videos are listed as direct MP4s.** By default gallery-dl hands
  every DASH video (nearly all reels) over as `ytdl:<post>/<n>.mp4`, a token
  for its own downloader that means nothing outside that process. The grid
  queued it as is, and the download went to gallery-dl and yt-dlp as if it
  were a post. Listings now ask for `videos=merged` (the progressive MP4, with
  audio, and its real size), and the parser replaces any `ytdl:` entry with
  its direct link or leaves it out. Whole-profile downloads from Downloads are
  unchanged: there gallery-dl has the manifest and its bundled yt-dlp can
  merge the best quality — verified that the binary the app installs includes
  it.
- **X advice that matches the actual error**, with gallery-dl's own messages:
  «authenticated cookies needed» and «Could not authenticate you» (session:
  the `auth_token` cookie of `.x.com`; old `twitter.com` cookies do not
  count), protected accounts, locked accounts, and the errors typical of an
  outdated gallery-dl (1.32.12 fixed X's per-request key, 1.32.15 detects the
  login redirect). The site advice now also reaches **listings**; it used to
  be applied only to downloads.
- **X video resolution is the file's**: gallery-dl reports the size of the
  uploaded original, and the file it delivers is often smaller — 135 of 262
  videos in the sample said 3840×2160 for a 1920×1080 file. The size now comes
  from the file URL. X's tweet id, a number, is kept as the post id, so file
  names carry it instead of a hash.
- An empty X listing explains its three possible causes (no media of its own,
  outdated gallery-dl, no x.com session) with the version numbers.

### Weibo and Threads: what the grid recognised

Checked against real gallery-dl listings from Weibo (24 October 2026: an
artist's photo album, a news account's video tab and two albums with Live
Photos, 167 files) and against the Threads capturer.

- **Weibo tiles knew nothing about their post.** gallery-dl puts the whole
  post inside `status`, and the generic parser only looked one level down and
  only for text. Every tile came out without post id (`status.id` is a
  16-digit number), without author (`status.user`), without carousel position
  (`status.count`), with the post text in HTML (`status.text` carries hashtags
  as links; `text_raw` is plain) and with the CDN link as its "page". The five
  measured listings now have no tile without post or author, and multi-image
  posts show `3/9`.
- **An expired Weibo video was retried with the same dead link.** Weibo signs
  video and Live Photo links for about an hour (`Expires=`, measured). The
  retry used the "page", which was that link. It now uses the post,
  `weibo.com/<uid>/<mblogid>`.
- **And the retry fetches only that file.** On Instagram and Weibo, where the
  listing and the retry come from the same gallery-dl extractor and number
  files the same way, the retry passes `--range N`: an expired file of a
  nine-photo post no longer downloads the other eight, once per expired file.
  If the range writes nothing, it repeats with the whole post rather than
  reporting an empty success. Kemono, Patreon and Facebook keep the whole-post
  retry, because their numbering does not match.
- **Weibo thumbnails were the originals.** The grid downloaded the full
  `large` JPEG (several MB for a 1100×5609 image) to draw a tile; it now uses
  the 360-px variant Weibo's API publishes for each photo. Videos take their
  cover from `page_info`, and a Live Photo's `.mov` takes its still photo;
  both used to be a ▶ with no image.
- **A Live Photo's video could not be downloaded.** It is listed as
  `video.weibo.com/media/play?livephoto=…`, which redirects to the `.mov`.
  Being a subdomain of weibo.com, it was routed to gallery-dl, which has no
  extractor for that path. It now goes to the HTTP engine, which follows the
  redirect.
- **Threads videos were saved as `.jpg`.** Meta serves many videos from paths
  without an extension (`/o1/v/t16/f2/m69/AQ…`). The capturer already said
  they were videos, but the extension stored for the grid fell back to `jpg`,
  and the grid passes it on as the real one when queueing. They are `.mp4`
  now, and Threads carousels show their position like Instagram's.
- **The same fallback broke Douyin and TikTok video posts sent to the grid.**
  The capturer sends a video post as its page (`…/video/<id>`) for yt-dlp to
  resolve; with `jpg` as its "real" extension it went to the HTTP engine
  instead, which saved the HTML page as a photo. Single posts go to the grid by
  default (*Single posts go to the selection grid*), so this was the common
  case. The page now has no extension and goes to yt-dlp.
- A single Weibo post pasted in Downloads goes to its author's folder
  (`weibo_<uid>`), as a profile already did, and the grid names the folder
  after the author with the most tiles rather than the first one: a profile
  that started with a retweet was filed under the retweeted account.

### Found in an independent review of all the new code

- **Patreon collections and creators with more than 100 posts showed only the
  last batch.** Every API page was sent to the grid as "page 1", which the
  grid treats as "replace". Collections had this since v1.8.8; creators
  inherited it with the native listing. Each batch now carries its own number
  and adds to the grid.
- **A creator URL with `?filters[tag]=…` listed everything.** The native
  listing ignored the filters; such URLs now go through gallery-dl with
  `id:<campaign>`, which applies them.
- **An expired Patreon link could be refreshed with another file's URL**:
  matching by suffix made `…_11.zip` match `1.zip`. It now rebuilds the exact
  name and refuses when more than one file would match.
- **Concurrent cookie lookups clobbered each other's temporary file**, which
  could cache "no session" for two minutes. One lookup per browser and domain
  at a time, each with its own file.
- **The Kemono probe armed the outage breaker on a 404 of one file**; it now
  does so only for timeouts, dropped connections and 5xx.
- **Pausing a Kemono row during the outage did not pause it**; it now stops
  between attempts and before the preview.
- **«Load more» on a single Kemono post duplicated every file**; it is shown
  only for creators.
- **A `%` followed by a non-ASCII character in a Kemono URL (`100%に`) closed
  the application**, and `&`, `#` and `+` in the tab's filter fields were
  mangled. Both fixed, with tests.

## Smaller things

- The supported-sites panel in Profile lists Kemono and Coomer, and the note
  under the URL field explains the 50-post batches when one of their links is
  pasted.
- `Load more` is shown for Kemono as it was for V2PH; everything else still
  lists in one streaming pass.
- A new `kemono.rs` module carries URL classification, API addresses, post and
  listing parsing, the creator search and the `Accept` quirk, with tests
  against real API responses saved in `src/testdata/`. Two network tests
  (`cargo test -- --ignored kemono_api patreon_api`) check the live contracts:
  the `Accept` header, the compressed responses, the vanity lookup, and that
  the creator page really does answer with the challenge.
