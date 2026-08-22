# Todo Downloader v1.8.5

**Hugging Face models, several connections per file, and profiles that stop
hiding posts**

Builds on v1.8.0. See `RELEASE-NOTES-v1.8.0.md` for artist discovery, Patreon,
Fanbox and the background slideshow. Notes for earlier versions are published on
the [Releases page](https://github.com/AcidClawX41/todo-downloader/releases);
only the current and previous ones are kept in the repository.

---

## Hugging Face model repositories

A model is not a file. `Qwen/Qwen3-32B` is seventeen shards of nearly 4 GB
each, plus `config.json`, `tokenizer.json`, `vocab.json`, `merges.txt` and
`model.safetensors.index.json`. Twenty-two copy-and-pastes from the *Files and
versions* tab is not a reasonable way to use a download manager, and it invites
the classic mistake: sixty gigabytes of weights and no index, so nothing loads.

Paste a repository address into the **Profile** tab and its files are listed
with their sizes. Hugging Face publishes the tree in an open API that needs no
token:

```
GET https://huggingface.co/api/models/Qwen/Qwen3-32B/tree/main?recursive=true
[{"type":"file","size":3957109648,"path":"model-00001-of-00017.safetensors"}, …]
```

Above the grid: `29 of 32 files ticked: 51.8 GB of 51.8 GB.` Thirty-two file
names do not tell you whether what you marked is four gigabytes or sixty.

**What comes ticked** is the complete model: weights plus config, tokenizer and
index. The rule works by exclusion, not inclusion — every architecture invents
its own configuration files (`preprocessor_config`, `chat_template.jinja`, the
`modeling_*.py` that `trust_remote_code` needs), so a whitelist would fall short
every couple of months and leave out something essential.

**Alternatives are not ticked**, and only where that is objective: the same
weights in `.bin` and `.safetensors` (only the second is ticked), and several
GGUF quantisations (none is). A subdirectory is deliberately *not* treated as a
variant — in a diffusion model `transformer/`, `text_encoder/` and `vae/` are
components and all three are needed.

**Gated repositories are caught before anything is queued.** Hugging Face lets
you list the files but will not serve them; without a check, fifty rows would
fail one by one with a 403 that never mentions the licence. `?expand[]=gated`
returns 66 bytes instead of the ~15 KB of the full object.

**The token is optional**, in Settings. Public models download without it, but
Hugging Face's own response says `unauthenticated; Please set a HF_TOKEN to
enable higher rate limits and faster downloads`. It travels in the
`Authorization` header, only to `huggingface.co` and `hf.co`, never on the
command line and never in a diagnostic.

## Several connections per file

A browser downloads over one connection. On a CDN that rarely saturates the
line: the ceiling is that TCP flow, not your bandwidth. Settings → Downloads →
Connections per file, four by default.

Three restrictions, all deliberate:

- **Proof, not a promise.** `accept-ranges` is not enough: one byte is requested
  and a `206` with `content-range` is required. A server that advertises ranges
  and ignores them would return the whole file to every thread — eight copies
  overwriting each other, and a corrupt file with no error on screen.
- **Only files that justify it.** The extension is checked before spending the
  probe. Against three hundred booru thumbnails those would be three hundred
  wasted round trips. Images take the same path as before, byte for byte.
- **A clean fallback.** Any "no" above returns to the single-connection path.

A segmented `.part` has holes, so its size says nothing about progress; a
`.tdseg` file records each segment's cursor. `Accept-Encoding: identity` is not
optional here: `reqwest` is built with gzip, and a compressed partial response
would put decompressed bytes at an offset described in compressed ones. Before
the final rename, every segment is checked as complete and the size against the
expected total.

## Profiles that were hiding posts in silence

`weibo.com/7187265342/QvRHJ0FYJ` returned `[]` with exit code 0 and an empty
stderr. Those posts are reposts, and the images live in the quoted post.
gallery-dl ships several switches off that discard content, and every one of
them logs at `debug`, so none reaches stderr:

```python
self.retweets = self.config("retweets", False)   # weibo.py:36
self.movies   = self.config("movies",   False)   # weibo.py:39
self.likes    = self.config("likes",    False)   # weibo.py:40
```

X has `retweets`, `quoted` and `pinned` — the last being the most galling: the
pinned tweet is the first thing on a profile and it was not even requested.
Bluesky has `reposts` and `quoted`, and enabling them also switches the profile
extractor from the media tab to the posts feed.

All of them are on now, **on both paths**. A switch set on only one makes the
profile list correctly and then download empty, which looks as if the listing
lied.

Not enabled, and not by oversight: `cards` (thumbnails of external links),
`ads`, `twitpic` (dead since 2017, one request per attempt) and `unavailable`.
A test checks those four stay off.

## Fixes

**AI model weights were saved as `.mp4`.** Not the routing: `url_extension`
capped extensions at five characters and `safetensors` is eleven, so it was
never recognised. Paths now allow twelve; the query stays at five, where X's
`format=` convention lives.

**The grid shows the file name.** Thirty-two identical rectangles with the name
in a tooltip are not a list. **`3783.0 MB` reads as `3.7 GB`**, and the
resolution is no longer printed as `—` for files that do not have one.

**Files keep their own name.** The repository path was flattened into the name,
combined with the author and truncated at 110 characters, so
`hunyuanimage2.1_refiner_fp8_e4m3fn.safetensors` arrived as
`Comfy-OrgHunyuanImage_2.1_ComfyUI_split_filesdiffusion_…_sp.safetensors`.
Not an ugly name: a file that does not serve what it was downloaded for. Each
file is now named after itself, and the folder is prefixed only where two files
in the repository share a name.

**No "links expire within hours" on Hugging Face.** True for social-network
CDNs; `/resolve/` is stable and signs a fresh URL on every request.

**The build script names the failing test.** The harness summary scrolls off the
console and only cargo's `error: test failed` survives. On failure it re-runs
with `--no-fail-fast`, writes `test-failures.txt` and prints the names and the
`panicked at` lines.

## Known limitations

- **A repository with several precision variants in one folder ticks all of
  them.** `Comfy-Org/Qwen-Image_ComfyUI` offers eight diffusion models — bf16,
  fp8_e4m3fn, fp8_hq, fp8mixed, nvfp4 — and only the `.bin`-vs-`.safetensors`
  and GGUF cases are detected. That listing marks around 260 GB. Use *Select
  none*.
- **Multi-connection applies only where the server proves it supports ranges**,
  and only to heavy files by extension. Set connections to 1 if a host
  rate-limits you for opening several.
- Everything listed under v1.8.0 and earlier still applies.

## Licence

[GPL-3.0-or-later](LICENSE) © 2026 Eric Valls Gramunt
