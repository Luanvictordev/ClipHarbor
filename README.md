# ClipHarbor

Self-hosted media downloader with a Flask UI for batching links, choosing formats, and saving MP4 or MP3 files from one queue.

**Repository:** [github.com/Luanvictordev/ClipHarbor](https://github.com/Luanvictordev/ClipHarbor)

![Python](https://img.shields.io/badge/python-3.10+-blue)
![License](https://img.shields.io/badge/license-MIT-green)

## Features

- Download media from 1000+ supported sites through [yt-dlp](https://github.com/yt-dlp/yt-dlp)
- MP4 video downloads and MP3 audio extraction
- Resolution picker for video downloads
- Batch URL support with automatic deduplication
- Refreshed responsive interface with a queue-first layout
- Vercel-friendly synchronous download flow with no background worker state

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

1. In the Vercel dashboard: **Storage** → **Create Database** → **Blob** → attach it to this project (or use **Connect Project** on an existing store).
2. Redeploy so **`BLOB_READ_WRITE_TOKEN`** appears in **Settings → Environment Variables** for Production (and Preview if you use it).
3. **Function duration**: in the Vercel dashboard, **Project → Settings → Functions**, raise **Max Duration** as high as your plan allows (Hobby is often capped around **60** s; longer downloads may need a paid plan or self-hosting). Do not add a `functions` entry in `vercel.json` for root `app.py`—it only matches the `api/` folder and breaks the build.

Locally, you can omit `BLOB_READ_WRITE_TOKEN`: the app serves files directly with `send_file` as before.

### YouTube limitations (no “magic” fix)

YouTube is built to limit automated access from **datacenter IPs** (including Vercel). There is **no** stable, free, set-and-forget method that never changes and uses **nothing** of yours—cookies, residential IP, or paid third-party services are the realistic options the ecosystem uses.

**Practical approach without uploading your cookies to the cloud:** run ClipHarbor **on your own computer** at home. `yt-dlp` often works on **residential** networks without extra configuration.

**If you must host in the cloud:** expect failures unless you supply **cookies** (they expire; see [exporting YouTube cookies](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies)) via `YT_DLP_COOKIES_FILE` or `YT_DLP_COOKIES_B64`.

**Power users:** optional `YT_DLP_YOUTUBE_EXTRACTOR_ARGS` (e.g. `youtube:player_client=android,tv`) may help briefly until YouTube changes again—see [yt-dlp usage](https://github.com/yt-dlp/yt-dlp#usage).

Keep `yt-dlp` updated; YouTube changes frequently.

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
- Download engine: yt-dlp + ffmpeg provided through `imageio-ffmpeg`

## Disclaimer

This tool is intended for personal use only. Respect copyright laws and the terms of service of the platforms you download from.

## License

[MIT](LICENSE)
