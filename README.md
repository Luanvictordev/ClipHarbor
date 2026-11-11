# ClipHarbor

Self-hosted media downloader with a Flask UI for batching links, choosing formats, and saving MP4 or MP3 files from one queue.

**Repository:** [github.com/Luanvictordev/ClipHarbor](https://github.com/Luanvictordev/ClipHarbor)

![Python](https://img.shields.io/badge/python-3.10+-blue)
![License](https://img.shields.io/badge/license-MIT-green)

## Features

- Download media from 1000+ supported sites through [yt-dlp](https://github.com/yt-dlp/yt-dlp)
- Spotify track, album and playlist downloads with automatic metadata and album artwork
- MP4 video downloads and MP3 audio extraction
- Resolution picker for video downloads
- Batch URL support with automatic deduplication
- Refreshed responsive interface with a queue-first layout
- Vercel-friendly synchronous download flow with no background worker state
- Spotify links are recognized and show the official offline-listening option

## Repository layout

| Path | Purpose |
|------|---------|
| `app.py` | Flask app, `/api/info`, `/api/download`, Blob upload on Vercel |
| `templates/index.html` | Single-page UI |
| `public/` | Static assets (favicon) |
| `requirements.txt` | Python dependencies |
| `runtime.txt` | Python version on Vercel |
| `Dockerfile` / `Procfile` | Container & process examples |

Copy `.env.example` to `.env` if you need local overrides (optional).

## Local Development

```bash
python3 -m venv venv
source venv/bin/activate   # Windows: venv\Scripts\activate
pip install -r requirements.txt
python app.py
```

Or use `./clipharbor.sh` on macOS/Linux.

Open `http://localhost:8899`.

## Deploy To Vercel

Flask is supported directly on Vercel with zero-config detection for `app.py`.

```bash
npm i -g vercel
vercel
```

### File downloads (Blob storage)

Vercel Functions cannot return media files larger than about **4.5 MB** in one response. After a download finishes, the app uploads the file to **[Vercel Blob](https://vercel.com/docs/vercel-blob)** (free tier on the Hobby plan) and the browser opens the Blob **download URL** instead.

1. In the Vercel dashboard: **Storage** → **Create Database** → **Blob** → attach it to this project (or use **Connect Project** on an existing store). The Blob SDK is installed for Vercel deployments and is optional on other hosts.
2. Redeploy so **`BLOB_READ_WRITE_TOKEN`** appears in **Settings → Environment Variables** for Production (and Preview if you use it).
3. **Function duration**: in the Vercel dashboard, **Project → Settings → Functions**, raise **Max Duration** as high as your plan allows (Hobby is often capped around **60** s; longer downloads may need a paid plan or self-hosting). Do not add a `functions` entry in `vercel.json` for root `app.py`—it only matches the `api/` folder and breaks the build.

Locally, you can omit `BLOB_READ_WRITE_TOKEN`: the app serves files directly with `send_file` as before.

### YouTube limitations (no “magic” fix)

YouTube is built to limit automated access from **datacenter IPs** (including Vercel). There is **no** stable, free, set-and-forget method that never changes and uses **nothing** of yours—cookies, residential IP, or paid third-party services are the realistic options the ecosystem uses.

**Practical approach without uploading your cookies to the cloud:** run ClipHarbor **on your own computer** at home. `yt-dlp` often works on **residential** networks without extra configuration.

When ClipHarbor runs on your own computer it also uses the cookies already in your browser: it looks for a Firefox, Chrome, Edge, Brave or Chromium profile and passes it to `yt-dlp` with `--cookies-from-browser`, so there is nothing to export. This happens only outside a cloud host (`RENDER`, `VERCEL`, `DYNO` and similar variables disable it) and can be pinned with `YT_DLP_COOKIES_FROM_BROWSER=firefox` or turned off with `CLIPHARBOR_NO_BROWSER_COOKIES=1`. If the cookies cannot be read (Chrome's App-Bound Encryption on Windows, a locked profile), the download retries without them instead of failing.

**If you must host in the cloud:** expect failures unless you supply **cookies** (they expire; see [exporting YouTube cookies](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies)) via `YT_DLP_COOKIES_FILE` or `YT_DLP_COOKIES_B64`.

**Power users:** optional `YT_DLP_YOUTUBE_EXTRACTOR_ARGS` (e.g. `youtube:player_client=android,tv`) may help briefly until YouTube changes again—see [yt-dlp usage](https://github.com/yt-dlp/yt-dlp#usage).

Keep `yt-dlp` updated; YouTube changes frequently.

### Spotify support

Spotify serves no downloadable audio, so ClipHarbor reads the track metadata from Spotify's public embed page (artist, title, duration, cover artwork, and the track list for playlists/albums) and then finds the audio on a searchable source, tagging the MP3 with the Spotify artwork and tags.

The audio source is chosen by `SPOTIFY_AUDIO_PROVIDERS` (default `soundcloud,youtube`), tried in order:

| Provider | Works on a cloud host? | Notes |
|----------|------------------------|-------|
| `soundcloud` | **Yes** | Does not block datacenter IPs. Many mainstream tracks only have a 30-second preview or a remix there, so some songs will not be found. |
| `youtube` | Only with cookies | Best catalog, but blocks anonymous downloads from datacenter IPs (“Sign in to confirm you're not a bot”). Add `YT_DLP_COOKIES_FILE`/`YT_DLP_COOKIES_B64` to use it on a server, or self-host on a home connection. |

The matching step compares the Spotify duration and title with each search hit, so 30-second previews, sped-up/slowed/remix uploads are rejected. When a source fails (for example DRM-protected), the next candidate is tried.

Playlists and albums are downloaded track by track into a ZIP. Tracks that could not be found are listed in `faixas-nao-baixadas.txt` inside the ZIP instead of failing the whole download.

Requirements: `ffmpeg`, which the `Dockerfile` installs system-wide and local installs get through `imageio-ffmpeg`. No Spotify API key, spotDL, or Deno runtime is needed.

### Desktop app (Windows)

`desktop/` holds a small native app for running downloads on your own machine: it needs no Python and no browser tab, uses your browser's cookies for YouTube, and writes MP3s with the Spotify tags and cover artwork. The executable is built by the **Desktop app (Windows)** GitHub Actions workflow — download it from the run's artifacts, unzip and run. See [desktop/README.md](desktop/README.md).

### Hosting on Render (Free plan)

- The Free plan gives **512 MB** of RAM, so avoid reintroducing heavy optional dependencies in the request path: spotDL used roughly **165 MB** just by being imported, and one download could push the service into an out-of-memory restart (the log shows `Handling signal: term` right after the download starts, and the browser reports `Download failed`).
- `Procfile` uses a **900 s** gunicorn timeout so multi-track playlists can finish.
- `ffmpeg` comes from `imageio-ffmpeg`; the Python runtime does not need apt packages. If you deploy the `Dockerfile` instead, system `ffmpeg` is already installed.
- A Free web service spins down after 15 idle minutes and takes about a minute to wake, so the first download after idle is slower.

## Docker

```bash
docker build -t clipharbor .
docker run -p 8899:8899 clipharbor
```

## Usage

1. Paste one or more media URLs into the dock.
2. Choose `MP4` for video or `MP3` for audio.
3. Click `Scan Links` to load metadata and available quality options.
4. Download individual items or use `Download All`.

## Stack

- Backend: Python + Flask
- Frontend: Vanilla HTML, CSS, and JavaScript
- Download engine: yt-dlp + ffmpeg (system binary or `imageio-ffmpeg`)
- Spotify metadata: public Spotify embed payload (no API key)

## Disclaimer

This tool is intended for personal use only. Respect copyright laws and the terms of service of the platforms you download from.

## License

[MIT](LICENSE)
