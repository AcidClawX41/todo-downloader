# Licensing history

**Todo Downloader v1.5.0 and later are licensed under GPL-3.0-or-later**
(see [LICENSE](LICENSE)).

**Versions up to and including v1.4.0 were released under the MIT License.**
Those releases remain MIT-licensed and can still be used under those terms —
a licence already granted cannot be revoked. The change applies to v1.5.0
onwards only.

## Why the change

MIT lets anyone take this code, close it and redistribute it without giving
anything back. GPL-3.0 keeps the project — and any derivative of it — free
and open for whoever receives it.

## Third-party components

| Component | Licence | How it is used |
|---|---|---|
| [librqbit](https://github.com/ikatson/rqbit) | Apache-2.0 | Compiled in (BitTorrent engine) |
| [egui / eframe](https://github.com/emilk/egui) | MIT / Apache-2.0 | Compiled in (GUI) |
| [tokio](https://tokio.rs), [reqwest](https://github.com/seanmonstar/reqwest), [image](https://github.com/image-rs/image) | MIT / Apache-2.0 | Compiled in |
| [yt-dlp](https://github.com/yt-dlp/yt-dlp) | Unlicense | Separate binary, invoked as a subprocess |
| [gallery-dl](https://github.com/mikf/gallery-dl) | GPL-2.0 | Separate binary, invoked as a subprocess |
| [ffmpeg](https://ffmpeg.org) | LGPL / GPL | Separate binary, invoked as a subprocess |
| [cyberdrop-dl-patched](https://github.com/NTFSvolume/cdl) | GPL-3.0-only | Optional, installed by the user |

All permissive (MIT/Apache-2.0) dependencies are compatible with GPL-3.0.
The engines are **not** bundled: they are downloaded by the user and run as
separate processes, so their licences do not extend to this codebase.
