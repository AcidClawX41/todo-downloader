# Todo Downloader v1.7.0

**X, Facebook, Bluesky and Threads profiles, and a way to stop a listing**

Builds on v1.6.7. See `RELEASE-NOTES-v1.6.7.md` for single-post capture and the
Windows installer. Notes for earlier versions are published on the
[Releases page](https://github.com/AcidClawX41/todo-downloader/releases); only
the current and previous ones are kept in the repository.

---

## X, Facebook and Bluesky profiles

Pasting `https://x.com/someone` into the Profile tab answered *Unsupported URL*.
gallery-dl has supported X for years — its extractor is still named `twitter`
but its root is already `https://x.com` — and it covers profiles, media, likes,
lists and searches. The application simply never routed anything there.

### Adding it meant fixing the routing first

The site list was matched with **substrings**. And `"x.com"` is inside
`linux.com`, `netflix.com`, `vox.com` and `box.com` — so adding it as written
would have sent all four to gallery-dl. It is the same shape of bug that once
let `passport.weibo.com` through for containing `weibo.com`.

**The routing now compares hosts.** Boorus keep a separate keyword list, because
`danbooru.donmai.us` does not carry the name as its whole domain — but those
keywords are searched **only inside the host**, never in the full URL, so a path
like `/wiki/danbooru` no longer changes the engine.

Two tests pin both sides down: X, Twitter, Facebook and Instagram are routed;
Linux, Netflix, Vox, Box and `x.com.attacker.net` are not.

### They are browsed with previews, not downloaded blind

Being supported by gallery-dl is not enough to get a selection grid. Browsing
means **listing now and downloading later**, and that only works if the file
links survive the wait.

- **X** qualifies cleanly. The listing carries `width`, `height` and the
  `pbs.twimg.com` URL with `name=orig` — the original — and that CDN is
  **public**: the link downloads without a session. The session is needed to
  *list*, not to *fetch*. **Bluesky** is the same: `cdn.bsky.app` is public and
  unsigned.

- **Facebook needed work first.** Its links are signed and expire, and the
  application has a safety net for exactly that — a dead direct link is
  re-resolved from the post page. But that net did not catch Facebook: its
  extractor publishes no `post_url` or `permalink`, and its `url` key *is* the
  CDN link, so the fallback would have retried the URL that had just died. It
  does publish the photo `id`, and the page is `facebook.com/photo/?fbid=<id>`,
  so it is now reconstructed. The chain is complete: dead link → post page →
  gallery-dl resolves it again.

**Threads has its own section below.** No extractor covers it, in gallery-dl or
in yt-dlp, so it could not be routed like the others — it needed a different
answer entirely.

### Facebook is slow, and the reason is worth stating

Its extractor cannot enumerate in bulk. `extract_set` walks the photos one at a
time, requesting the **full HTML page of each one** to get its URL and the id of
the next. Thirty photos are thirty sequential page loads of facebook.com.

X is instant by comparison because its extractor uses the GraphQL API and
returns a whole batch of posts, with all their media, in **one** response. The
difference is not in this application; it is in what each site allows.

Two attempts at making it fast were wrong, and both are worth recording:

- **Smaller batches made it worse.** The extractor has no random access: to give
  you items 9–16 it walks 1–16 again. Paging by eight costs 8 + 16 + 24 + 32 —
  for forty photos, a hundred and twenty requests instead of forty. The batch is
  now 24, because larger batches are cheaper per item, not more expensive.

- **Removing the background chain achieved nothing.** That walk is paid
  identically when *you* press *Load more*, so the only result was making you
  press it. Three batches are now fetched automatically — 24, 48 and 72 items —
  and it stops there rather than running to the forty pages other sites use.

There is no version of this that is fast. The application no longer does
unnecessary work, and the interface now says how long it will take and why.

### The Profile tab lists what it accepts

A collapsible **Supported sites** panel, in the interface language: which sites
give a preview grid, which are downloaded whole, which need your session, which
only work through the Capture tab, and which are not supported at all. Folded by
default — five lines that are read once should not be five lines you scroll past
forever.

---

## Threads, at full resolution

Threads is the one site in the target set with **no extractor anywhere**.
gallery-dl 1.32.9 has none — 285 extractor files, not one matching `threads.net`
or `threads.com`. yt-dlp's request, [#7523](https://github.com/yt-dlp/yt-dlp/issues/7523),
has been open since July 2023 with its pull request unmerged.

### Why the obvious approach does not work

Douyin's single-post capturer reads the rendered page and rewrites ByteDance's
`~tplv-…` thumbnail path into `~noop`, the untouched original. That trick is
worth a lot and it does not transfer: **Meta signs its CDN links**. You cannot
take the 320-pixel copy the browser is displaying and derive the 1440-pixel one
from it. The full-resolution URL exists in exactly one place — the JSON the page
receives — and reading the DOM never sees it.

### Why the second approach was rejected too

Requesting that JSON ourselves would mean reproducing Meta's `doc_id`, `lsd` and
`X-IG-App-ID`. The `doc_id` changes with their deploys, so the extractor would
break without warning, and the breakage would look like *"the profile is
empty"* — the exact failure mode this project has been bitten by before.

### What it does instead

The script does not build a request. It reads the responses **the page itself
already received**, walking the JSON by shape rather than by path: a publication
is anything carrying media *and* something identifying it. Meta can rename its
containers and this keeps working, because when they change the API they change
their own client with it.

From each publication it takes the largest entry of `image_versions2.candidates`
and of `video_versions` — sorted by **area**, since 1080×1350 and 1080×1080 tie
on width and are not the same file. Carousels are split into their individual
photos, and the post's cover is not counted as a fourth file.

Three details that were only visible once the payloads were measured rather than
guessed at:

- **A profile response is not only that profile.** One response from a
  four-post profile carried **eighty-one** media blocks: Threads mixes in
  recommendations from other accounts. Without a filter on the handle, the grid
  filled with files nobody asked for.
- **Every media node carries both keys.** `video_versions` being empty is how an
  image looks, not a missing field — so the type comes from the data and not
  from guessing at a URL that often has no recognisable extension.
- **The thumbnail is deliberately the smallest candidate.** The grid draws 320
  pixels; fetching the 1440-pixel original for that would be tens of megabytes
  per profile.

Results land in the **Profile** grid with their real resolution, ticked one by
one — not dumped into the queue. And the resolution shown is the file's own,
because it now travels from the API instead of being measured off a thumbnail
that was shrunk on purpose.

### Where you run it

Two ways, and one of them is clearly better on Threads:

- **As a userscript** (Tampermonkey or Violentmonkey) — the same install that
  already adds *"⬇ Capture this post"* to Douyin and TikTok now adds
  *"⬇ Capture this profile"* on Threads. Recommended, for two reasons that are
  not about convenience. It runs at `document-start`, so it is listening before
  the page requests anything — on Threads there is nothing to read from the DOM
  afterwards, so a response that arrives unobserved is lost until you reload.
  And it uses `GM_xmlhttpRequest`, which reaches the application on Chrome and
  Vivaldi, where a plain page cannot talk to `127.0.0.1`.
- **Pasted into the console**, like the TikTok and V2PH scripts. Works, and on
  Chrome or Vivaldi falls back to saving a JSON you import.

Pasting a Threads URL into the **Profile** tab used to produce a bare
`Unsupported URL` — true, but useless. It now opens the Capture tab on the right
script and says why.

It needs the browser open on the profile, and it uses **your** session, address
and TLS fingerprint. It is not a native extractor and is not presented as one.

## A Stop button for the listing

A Facebook profile can take minutes to list, and until now the only way to stop
it was to close the application. There is now a **■ Stop** next to the spinner,
and **Clear list** stops the listing too — emptying a list while leaving
gallery-dl working to refill it is not what that button says it does.

Bumping the epoch was not enough on its own. The epoch discards the *results*,
but the process keeps running and keeps requesting pages: on Facebook that is
minutes of traffic nobody is waiting for, and with the application closed those
requests would be orphaned.

So the cancellation reaches the process. It is polled every 120 ms and kills the
**tree**, not just the parent — gallery-dl is Python and can have children of
its own, and killing only the parent would leave a grandchild talking to
Facebook.

Two details that matter more than they look:

- **The flag is lowered when a new search starts.** Without that, the first stop
  would have left the browser useless forever — a silent failure and a miserable
  one to diagnose.
- **A stop is not an error.** The process ends with `ErrorKind::Interrupted` and
  that is where it stays. The epoch would already discard it, but sending it
  anyway would leave a red error primed for whenever someone changes that
  filter.

---

## Known limitations

- **Danbooru and AIBooru may answer the built-in Booru search with a Cloudflare
  challenge (403).** That is not a missing session. gallery-dl cannot solve a
  JavaScript challenge, and the `cf_clearance` cookie proving you passed one in
  a browser is tied to your IP *and* your User-Agent and lasts around half an
  hour. A freshly exported `cookies.txt` sometimes works and sometimes does not.
  **Pasting the tag-search URL straight into Downloads does work**, because that
  path downloads instead of querying the JSON API. The Booru tab now says this
  when it happens, instead of reporting a timeout.
- **Booru credentials are one pair, and they are Gelbooru's.** Gelbooru is the
  only site that requires them. They used to be sent to whichever booru you
  searched, which left Danbooru, AIBooru, e621 and Konachan hanging until they
  timed out — a Gelbooru `user_id` offered to Danbooru as a username is not a
  credential it can use. Per-site accounts are not implemented yet.

- **Facebook listing is slow and will stay slow.** One full page load per photo,
  sequentially, plus the pacing between requests. For a large profile it is
  quicker to paste the URL into Downloads and fetch the whole thing: the cost is
  the same, but files arrive one by one instead of after a wait.
- **Facebook's own extractor is among the most fragile in gallery-dl** and needs
  a session. If a listing comes back empty, that is usually the session.
- **Threads needs the browser.** There is no way to paste a Threads URL into the
  Profile tab and have it list: the script has to run in the page. Scrolling is
  the pagination, so a very large profile is a long scroll — the script does it
  automatically and stops when nothing new arrives.
- **Threads posts from other accounts are filtered out on purpose.** If you want
  a reposted item, open that account's profile and capture there.
- Everything listed in `RELEASE-NOTES-v1.6.7.md` still stands, including that
  **MEGA pause/resume mid-transfer remains untested** against real links.

## Licence

GPL-3.0-or-later. v1.4.0 and earlier remain available under MIT — see
`LICENSE-HISTORY.md`.
