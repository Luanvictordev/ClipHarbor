# ClipHarbor Desktop

A small native Windows app that downloads Spotify tracks/albums/playlists and
other media links on your own computer — no Python, no browser tab, no server.

Run it, paste a link, press **Baixar**. Downloads go to
`%USERPROFILE%\Downloads\ClipHarbor` by default.

## Why a desktop app

The hosted version can only reach SoundCloud (YouTube blocks datacenter IPs)
and SoundCloud has 30-second previews for much of the mainstream catalog. On
your own connection YouTube works, and with your browser cookies it stops
asking "Sign in to confirm you're not a bot".

## Getting the .exe

The binary is built by GitHub Actions, so you do not need a Rust toolchain:

1. Open the **Actions** tab of this repository → **Desktop app (Windows)**.
2. Pick the latest run → **Artifacts** → download `clipharbor-desktop-windows`.
3. Unzip it and run `clipharbor-desktop.exe`.

It is a single portable executable: no installer, no registry entries. To
remove it, delete the exe and the `%LOCALAPPDATA%\ClipHarbor` folder.

## How it works

The app is a native UI (egui) around the same ideas as the web version:

| Part | Implementation |
|------|----------------|
| Spotify metadata | The public embed payload of the track/album/playlist (name, artists, duration, cover, track list). No API key, no account. |
| Audio/video download | `yt-dlp`, which the app downloads on first use into `%LOCALAPPDATA%\ClipHarbor\bin` together with `ffmpeg`, so the exe stays small and the extractor stays updatable. |
| Matching | Search hits are ranked by duration and title tokens; 30-second previews, sped-up/slowed/remix/cover versions are rejected. |
| Tags | The MP3 gets title/artist/album from Spotify plus the 640×640 cover artwork. |

Rust has no maintained YouTube extractor, so the actual extraction is yt-dlp.
The native part owns everything else: the UI, cookie selection, Spotify
metadata, candidate ranking, tagging and the failure report.

### Cookies

Choose the browser you are logged into YouTube with (Firefox, Chrome, Edge,
Brave, Chromium) and `yt-dlp` reads its cookies directly — nothing to export.
On Windows, prefer **Firefox**: Chrome 127+ encrypts cookies with App-Bound
Encryption, which yt-dlp cannot decrypt.

Only browsers with a profile on this machine are listed, and if the cookies
still cannot be read (browser closed while its database is locked, empty
profile, undecryptable store) the app says so and continues without cookies
instead of failing every track. Firefox installed from the Microsoft Store is
detected too; for other portable installs, pick another browser or `nenhum`.

## Limitations

- First run needs internet to fetch `yt-dlp` (~17 MB) and `ffmpeg` (~100 MB
  compressed).
- YouTube can still rate-limit (`HTTP 429`) after many downloads in a row.
- Tracks without a matching upload are listed in `faixas-nao-baixadas.txt` in
  the output folder instead of failing the whole album.
- The exe is unsigned, so Windows SmartScreen may warn the first time.

## Building from source

```bash
cd desktop
cargo build --release
# deploy/target/release/clipharbor-desktop(.exe)
```

For testing without a GUI there is a CLI mode:

```bash
cargo run -- --cli "https://open.spotify.com/track/5XeFesFbtLpXzIVDNQP22n" /tmp/out
```

Environment overrides: `CLIPHARBOR_YTDLP`, `CLIPHARBOR_FFMPEG`,
`CLIPHARBOR_BIN_DIR`, `CLIPHARBOR_COOKIES` (CLI only).
