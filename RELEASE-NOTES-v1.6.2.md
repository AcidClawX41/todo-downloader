# Todo Downloader v1.6.2

**Native V2PH support, galleries that keep loading, and honest session handling**

Everything in v1.6.0 still applies — see `RELEASE-NOTES-v1.6.0.md` for the
MEGA.nz engine, the preview grids and the YouTube fix. This release builds on it.

---

## V2PH albums and profiles, natively

Paste a V2PH album or model URL into the Profile tab and pick what to download
from the preview grid. No external engine, no Python, no browser automation.

- **A whole album, not just the visible page.** An album is split into pages of
  ten photos; the extractor walks all of them, so a 38-photo album lists 38.
- **A whole model profile.** Each grid page is one complete album. Agency,
  category and country pages work the same way.
- **Original quality.** The URLs in the page already point at the originals —
  the same file "save image as" gives you. There is no downscaled variant to
  work around.

Downloads go through the native HTTP engine, so resume, pause, per-author
subfolders and the duplicate archive all apply.

### Why this is not an external engine

The obvious route was to bundle the existing Python downloader the way
`cyberdrop-dl` works. Probing the site first showed that was unnecessary for
listing: V2PH serves complete server-rendered HTML and the image URLs are in
the markup verbatim. Bundling Python, Selenium and a required Chrome
installation would have been the heaviest dependency in the project.

Only what is anchored to a URL pattern is parsed — the photo CDN path, album
links, the page number. Metadata labels are deliberately ignored because the
site translates them into ten languages, so a parser reading "Photos" breaks
for anyone browsing in Korean.

### What the site does push back on

Two limits are the site's, not the application's, and both are stated plainly
in the interface when they happen:

- **Past the tenth photo of an album, a session is required.** Point
  *Settings → Cookies* at a `cookies.txt` exported from a signed-in browser.
- **An account may only open so many albums per day.** The site shows a
  counter; already-opened albums can be revisited freely. Nothing here can
  raise that ceiling, so a large profile takes several days.

And one behaviour worth knowing: **V2PH rate-limits bursts with `403`**. An
analysis is a handful of requests, so it is easy to trip by retrying. Requests
are spaced 900 ms apart, and V2PH deliberately does **not** use the background
page-chaining described below — in Instagram each page is one request, here
each "page" is an entire album, and chaining four turned an analysis into
thirty consecutive requests. When a 403 does happen the message says what it is
and that waiting is the fix, rather than showing a bare status code.

### A browser-side script, for when it does push back

The **Capture** tab now has a **V2PH** script alongside TikTok and Douyin. Paste
it into the console on an album page and your own browser walks the album and
hands the URLs to the application.

This exists because the site's pushback happens below the HTTP headers, so no
header, cookie or User-Agent fixes it from outside. The script does not imitate
a browser — it runs in one, with your session, your address and your browser's
own TLS fingerprint. Downloading is unaffected either way: the image CDN is not
the part that pushes back.

Chrome blocks pages from reaching `127.0.0.1`, so there the script falls back to
saving a JSON file you import from *Downloads → Import TXT/JSON*. Firefox
delivers straight to the queue.

## Galleries keep loading in the background

Instagram profiles felt slow. Raising the page size would have made it worse:
requests to Instagram are deliberately spaced 6–12 seconds apart, so asking for
150 items at once means eight consecutive requests and over a minute of blank
screen, because nothing renders until the whole page returns.

Instead, the first 30 arrive as fast as before and **four more pages are fetched
silently behind them**. The grid grows while you look at it. Total time is
unchanged; it stops being dead time. *Load more* re-arms the chain. If a
background page fails, the chain stops quietly instead of throwing an error over
a grid that is working.

**Thumbnails now load eight at a time instead of four.** That limit protected
nothing: covers come from the CDN, not the rate-limited API.

## Sessions: what works, and what cannot

Three ways to give the application a session, in the order it tries them.

**In-app sign-in** (Settings → V2PH account). The password is sent to the site
and destroyed when the request returns; only the session cookie the site
returns is kept, exactly as a browser keeps it. Signing out deletes it. The
login form is read from the page rather than hardcoded, and success is verified
against the site — a 200 on a login page proves nothing, since many sites
re-serve the form on a bad password.

**On V2PH specifically this cannot work**, and the application now says so
instead of failing obscurely: the site protects its login page with Cloudflare's
bot challenge, which requires a real browser. Album pages are not challenged, so
a `cookies.txt` is the supported route there.

**Firefox cookies, read directly.** *Use browser cookies* now works for the
engines built into the binary, not just for yt-dlp and gallery-dl. The database
is copied to a temporary file first — Firefox locks it while running — and the
copy, with its `-wal` companion, is deleted immediately.

Two limits, both found in testing and both now documented rather than
discovered by users: Firefox writes only cookies **with an expiry date** to that
database, so a login issuing a session cookie cannot be picked up this way by
any external tool; and the App-Bound Encryption warning shown for Chromium
browsers **only applies on Windows** — on Linux and macOS those cookies are
readable, with a Keychain prompt on macOS. The warning is now shown only where
it is true.

**A `cookies.txt` file**, which takes priority over both and is the most capable
of the three: browser extensions read live cookies through a privileged API,
including session cookies and Cloudflare's `cf_clearance`.

## A User-Agent field, and a button that fills it

Settings → Browser cookies has an optional **User-Agent** field, and a **Detect
from my browser** button next to it.

When a browser passes a Cloudflare check, the `cf_clearance` cookie it receives
is bound to the IP **and to the User-Agent that earned it**. The application was
sending a fixed Chrome string, so a `cookies.txt` exported from Firefox carried
a clearance cookie that Cloudflare then discarded as mismatched.

The button opens a page served by the application's own local receiver; the
browser states its User-Agent in that request, and it is read from there. No
guessing at versions, no configuration files, and it works for any browser on
any system. For a browser other than your default, paste the same address into
it. Nothing is faked and no check is circumvented — this only stops the
application from misrepresenting itself and invalidating a permission you
already hold.

## Fixes

- **Clearing the gallery list crashed the application.** The button emptied the
  list mid-frame while the grid was still drawing from indices into it. The
  clear is now deferred until after drawing, which is the pattern the profile
  list already used.
- Thumbnails that cannot be fetched fall back to a media icon instead of
  showing a loading indicator forever, and are no longer retried endlessly.
- The gallery grid has a **Clear list** button, which the profile list already had.

## Known limitations

- V2PH previews download the full-size image, because the site has no smaller
  variant — the browser does the same to display an album. Previewing a large
  album is not free.
- V2PH's daily album quota and its ten-photo limit for visitors are the site's,
  and no route here changes them.
- Reading cookies from the browser covers Firefox only, and only persistent
  cookies. See above.
- Everything listed in `RELEASE-NOTES-v1.6.0.md` still stands, including that
  **MEGA pause/resume mid-transfer remains untested** against real links.

## Licence

GPL-3.0-or-later. v1.4.0 and earlier remain available under MIT — see
`LICENSE-HISTORY.md`.
