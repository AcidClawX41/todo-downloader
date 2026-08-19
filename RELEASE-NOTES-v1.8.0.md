# Todo Downloader v1.8.0

**Find the artists, not just the files**

Builds on v1.7.0. See `RELEASE-NOTES-v1.7.0.md` for X, Facebook, Bluesky and
Threads profiles. Notes for earlier versions are published on the
[Releases page](https://github.com/AcidClawX41/todo-downloader/releases); only
the current and previous ones are kept in the repository.

---

## Discover artists

A new tab. You type a character — `yukinoshita_yukino`, `tohsaka_rin`,
`bismarck_(azur_lane)` — and it answers with **the profiles that draw them**,
ranked by how often, each with four sample thumbnails and a button to send that
profile to the queue or open it in the grid.

```
🖼🖼🖼🖼   ponkan_8            19 posts of this character
          X  https://x.com/ponkan_8                      [Open] [＋ To queue]

🖼🖼🖼🖼   shou3719            16 posts of this character
          Fanbox 🔒  https://www.fanbox.cc/@shou3719     [Open] [＋ To queue]
```

### The index already exists, and it is not ours

This does not search X or Pixiv. It reads the **`source` field** of booru posts
— the link back to the artist's original post — and that turns a character tag
into a cross-reference table pointing at independent artists.

Measured on 300 posts of `wuthering_waves` on yande.re before any code was
written:

| Posts | Destination |
|------:|:--|
| 107 | pixiv.net |
| 76 | patreon.com |
| 46 | fanbox.cc |
| 43 | x.com |
| 11 | bilibili.com |
| **1** | *no source at all* |

**299 of 300 say where they came from.** That index is maintained by thousands
of taggers, updated daily, and costs nothing. Building a crawler to duplicate it
would have taken months and produced worse data — the classic mistake in this
space.

### Ranking by posts of *that* character filters itself

Someone who drew a character nineteen times matters more than a large account
that drew them once. And the same ordering sinks the noise without a blocklist
to maintain: `AMNIBUS_STORE` and `anime_oregairu` sit at the bottom with two and
three, under Ponkan⑧'s nineteen.

### One artist, all their houses

Artists reuse their name across sites, so `siino13` on Fanbox and `Siino_13` on
X are the same person. Grouped by a normalised identifier, they become one entry
with their posts summed and **both addresses listed** — the open ones first.

That is what makes the feature useful when a Fanbox needs a paid plan you do not
have: **their X is right underneath**, in green, and it works. Sites that charge
per creator are marked 🔒, because being subscribed to ten others does not open
that one.

### Depth is yours to choose

A slider, 1 to 10 pages. Measured on Yukino:

| Pages | Artists |
|------:|--------:|
| 3 | 17 |
| 5 | **33** *(default)* |
| 8 | 36 |

Going deeper more than doubles the result and what it costs is time, so it is a
choice rather than a constant. It stops on its own when the tag runs out, shows
*page 3 of 5* while it works, and has a **■ Stop** that kills the harvest rather
than only discarding it.

Konachan was tried as a second source and dropped: it has the posts but not the
attributions — 193 posts, zero profiles. Twice the requests for nothing.

### What it cannot do

**It only finds what somebody tagged.** An artist who publishes on X and whom
nobody uploads to a booru is invisible here, and no amount of extra pages
changes that. Shou's X exists; not one of the 721 posts under that tag cites it.

---

## Patreon, Fanbox and Pixiv

gallery-dl has covered all three for years. The application simply never routed
anything there.

- **Patreon** — a creator, a single post, a collection, or **`patreon.com/home`
  for every subscription you have at once**. It needs your session, and it only
  brings what your account can already see: the extractor checks
  `current_user_can_view` and skips the rest.
- **Fanbox** — browsed with previews, like Patreon. Needs the `FANBOXSESSID`
  cookie, which the application already supplies.
- **Pixiv** — downloaded whole, **not** browsed, and that is deliberate. Its
  extractor does not work from cookies: it requires an OAuth `refresh-token`
  obtained through a separate procedure that this application does not manage.
  Offering a grid that always comes back empty would be worse than what it does
  now.

### Patreon post titles broke Windows

Downloads failed with `[Errno 22] Invalid argument`, an error that mentions
nothing useful. gallery-dl's Patreon template is
`{id}_{title}_{num:>02}.{extension}`, and on Patreon a title is a whole
sentence:

```
166592250_Sorry I was gone for a while umm so ...noises now (_´～｀_)_01.png
```

A hundred and five characters of file name. In a short test folder it fits;
under `Downloads\Todo Downloads\<creator>\` it does not. The title is now capped
at sixty, keeping `id` and `num` — the parts that actually identify the file.

---

## X videos have thumbnails again

Videos in the profile grid showed a play triangle and no image. X's extractor
*does* have the poster, but only emits it when asked:
`videos_previews = self.config("previews", False)`.

Asking for it took three attempts and each one is worth recording:

1. The option went into the **download** path, where it did nothing for the grid
   *and* would have downloaded a stray JPEG next to every video.
2. Moved to the listing, it worked — and produced **two rows per video**, the
   `.mp4` and its poster as separate selectable files.
3. Reading the twenty lines of the extractor showed the poster arrives as its
   own entry directly behind its video. It is now paired: one row per video,
   with its image.

The first two were written by reasoning about what the option probably did. The
third came from reading it.

---

## A slideshow for the background

The custom background was one image, so in practice nobody ever changed it. It
is now three modes — **None · One image · Slideshow** — with folders, optional
subfolders, random or sequential order, an interval from one to sixty minutes
and a crossfade you can turn off.

Off by default. A background that moves on its own is a distraction, and this is
a tool.

Two details that are invisible when they work:

- **egui repaints on events, not in a loop**, so without an explicit
  `request_repaint_after` the background would only change when you moved the
  mouse. Continuous repainting is limited to the second the crossfade lasts —
  a download manager has no business burning GPU on decoration.
- **Folder scanning runs on its own thread**, not on the async runtime. It is
  blocking disk I/O, and taking a runtime thread would leave downloads waiting
  because of a wallpaper.

Intensity and blur moved to the foot of the card, below a separator: they apply
to all three modes, and sitting inside the single-image block suggested they
belonged to it.

---

## Fixes

**A cookie that expired is not a cookie that is missing.** gallery-dl says
`cookies: fanbox.cc/FANBOXSESSID expired at …` and then `no cookie set`. Reading
only the second sent you to check a session that was fine. The two cases now
read differently.

**A 403 on Fanbox is not a session problem either.** It means those posts
require a paid plan on *that* creator. On Fanbox «Following» is free and unlocks
nothing; «Support» is what grants access. The message says so instead of
pointing at your cookies.

**The download queue shows the command that failed**, with the cookie path
redacted — the same diagnostic the Booru tab got in v1.7.0. Without it, three
separate problems this cycle were diagnosed by guessing at arguments from
memory, and two of those guesses were wrong.

**Fifty-five example tags**, every one verified against the API before being
added. An example that returns nothing is worse than no example: the first
search fails and the feature looks broken. Both Asukas are there, and both
Bismarcks — same ship, two games, two unrelated tags.

---

## Known limitations

- **The artist finder only sees what boorus attribute.** No source, no artist.
  Pixiv artwork URLs do not name their author, so those posts are skipped rather
  than guessed at.
- **Pixiv is not browsable** and will not be until the `refresh-token` flow is
  handled. Pasting a Pixiv URL into Downloads works.
- **Fanbox and Patreon charge per creator.** Their profiles list, but posts
  behind a plan you do not hold return 403 — correctly, and the interface says
  which case it is.
- **Grouping artists by name is a heuristic.** Two different people choosing the
  same handle would be merged. The cost of being right — seeing the X of someone
  whose Fanbox you cannot open — outweighs it by a wide margin.
- Everything listed in `RELEASE-NOTES-v1.7.0.md` still stands, including the
  Cloudflare limits on Danbooru and AIBooru, Facebook's slow listing, Threads
  needing the browser, and **MEGA pause/resume remaining untested** against real
  links.

## Licence

GPL-3.0-or-later. v1.4.0 and earlier remain available under MIT — see
`LICENSE-HISTORY.md`.
