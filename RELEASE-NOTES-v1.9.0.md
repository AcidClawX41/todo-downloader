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
