import base64
import binascii
import glob
import json
import logging
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
import uuid
import zipfile
from contextlib import contextmanager
from pathlib import Path
from urllib.parse import urlparse

from flask import Flask, jsonify, render_template, request, send_file, send_from_directory
from flask_limiter import Limiter
from flask_limiter.util import get_remote_address
try:
    import vercel_blob
except (ImportError, ModuleNotFoundError):
    vercel_blob = None

# ---------------------------------------------------------------------------
# Logging
# ---------------------------------------------------------------------------
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
)
logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# App setup
# ---------------------------------------------------------------------------
app = Flask(__name__)

BASE_DIR = Path(__file__).resolve().parent
PUBLIC_DIR = BASE_DIR / "public"
APP_NAME = os.environ.get("APP_NAME", "ClipHarbor")

# ---------------------------------------------------------------------------
# Rate limiting  (memory store — fine for a single-instance free tier)
# ---------------------------------------------------------------------------
limiter = Limiter(
    key_func=get_remote_address,
    app=app,
    default_limits=["60 per hour"],
    storage_uri="memory://",
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
def is_youtube_url(url: str) -> bool:
    try:
        host = (urlparse(url).hostname or "").lower()
    except ValueError:
        return False
    if not host:
        return False
    return host == "youtu.be" or host.endswith(
        ".youtu.be"
    ) or host == "youtube.com" or host.endswith(".youtube.com")


def is_spotify_url(url: str) -> bool:
    """Recognize Spotify links."""
    try:
        host = (urlparse(url).hostname or "").lower()
    except ValueError:
        return False
    return host == "spotify.com" or host.endswith(".spotify.com")


def is_spotify_playlist_url(url: str) -> bool:
    """Recognize Spotify playlist or album links."""
    try:
        parsed = urlparse(url)
        host = (parsed.hostname or "").lower()
        if host == "spotify.com" or host.endswith(".spotify.com"):
            path = parsed.path.lower()
            return "/playlist/" in path or "/album/" in path
    except Exception:
        pass
    return False


SPOTIFY_EMBED_PAGE = "https://open.spotify.com/embed/{kind}/{spotify_id}"
SPOTIFY_LINK_REF = re.compile(r"/(track|playlist|album)/([A-Za-z0-9]+)")


def spotify_link_ref(url: str) -> tuple[str, str]:
    match = SPOTIFY_LINK_REF.search(urlparse(url).path)
    if not match:
        raise RuntimeError(
            "Link do Spotify não reconhecido. Use o link de uma faixa, álbum ou playlist."
        )
    return match.group(1), match.group(2)


def fetch_spotify_entity(url: str) -> dict:
    """Read Spotify's public embed page payload.

    It carries the track metadata (name, artists, duration, cover) and, for
    playlists/albums, the track list — with no API keys, no spotDL and no
    heavyweight Python dependencies.
    """
    kind, spotify_id = spotify_link_ref(url)
    request_obj = urllib.request.Request(
        SPOTIFY_EMBED_PAGE.format(kind=kind, spotify_id=spotify_id),
        headers={"User-Agent": "Mozilla/5.0"},
    )
    with urllib.request.urlopen(request_obj, timeout=20) as response:
        page = response.read().decode("utf-8", "replace")

    match = re.search(
        r'<script id="__NEXT_DATA__" type="application/json">(.*?)</script>', page, re.S
    )
    if not match:
        raise RuntimeError("O Spotify não devolveu os metadados deste link.")

    try:
        return json.loads(match.group(1))["props"]["pageProps"]["state"]["data"]["entity"]
    except (KeyError, TypeError, json.JSONDecodeError) as exc:
        raise RuntimeError("Não consegui ler os metadados do Spotify para este link.") from exc


def spotify_cover_url(entity: dict) -> str:
    sources = (entity.get("coverArt") or {}).get("sources") or []
    images = (entity.get("visualIdentity") or {}).get("image") or []
    candidates = [
        (item.get("width") or item.get("maxWidth") or 0, item.get("url"))
        for item in [*sources, *images]
    ]
    candidates = [(size, link) for size, link in candidates if link]
    if not candidates:
        return ""
    return max(candidates, key=lambda item: item[0])[1]


def spotify_track_info(track: dict, fallback_cover: str = "") -> dict:
    """Normalize a track from a Spotify embed payload."""
    name = (track.get("title") or track.get("name") or "").strip() or "Spotify Track"
    artists_list = track.get("artists") or []
    artists = track.get("subtitle") or ", ".join(
        artist.get("name", "") for artist in artists_list if artist.get("name")
    )
    artists = artists.strip()
    duration_ms = track.get("duration") or 0
    track_id = str(track.get("uri") or "").rsplit(":", 1)[-1]
    return {
        "url": f"https://open.spotify.com/track/{track_id}" if track_id else "",
        "title": f"{artists} - {name}" if artists else name,
        "thumbnail": fallback_cover,
        "duration": round(duration_ms / 1000) if duration_ms else None,
        "uploader": artists or "Spotify",
        "formats": [{"id": "mp3", "label": "MP3 Audio", "height": 0}],
    }


def get_spotify_track_info(url: str) -> dict:
    entity = fetch_spotify_entity(url)
    return spotify_track_info(entity, spotify_cover_url(entity))


def get_spotify_info(url: str) -> dict:
    if is_spotify_playlist_url(url):
        entity = fetch_spotify_entity(url)
        cover = spotify_cover_url(entity)
        tracks = [
            spotify_track_info(track, cover) for track in (entity.get("trackList") or [])
        ]
        return {
            "is_playlist": True,
            "title": entity.get("name") or "Spotify Playlist",
            "track_count": len(tracks),
            "tracks": tracks,
            "thumbnail": cover,
        }

    return get_spotify_track_info(url)


CLOUD_HOST_ENV_VARS = (
    "RENDER",
    "VERCEL",
    "DYNO",
    "K_SERVICE",
    "FLY_APP_NAME",
    "RAILWAY_ENVIRONMENT",
)

_browser_cookies_spec: str | None = None
_browser_cookies_resolved = False


def _local_browser_profile_dirs() -> dict[str, list[Path]]:
    home = Path.home()
    if sys.platform == "win32":
        local = Path(os.environ.get("LOCALAPPDATA") or home / "AppData" / "Local")
        roaming = Path(os.environ.get("APPDATA") or home / "AppData" / "Roaming")
        return {
            "firefox": [roaming / "Mozilla" / "Firefox" / "Profiles"],
            "chrome": [local / "Google" / "Chrome" / "User Data"],
            "edge": [local / "Microsoft" / "Edge" / "User Data"],
            "brave": [local / "BraveSoftware" / "Brave-Browser" / "User Data"],
            "chromium": [local / "Chromium" / "User Data"],
        }
    if sys.platform == "darwin":
        support = home / "Library" / "Application Support"
        return {
            "firefox": [support / "Firefox" / "Profiles"],
            "chrome": [support / "Google" / "Chrome"],
            "edge": [support / "Microsoft Edge"],
            "brave": [support / "BraveSoftware" / "Brave-Browser"],
            "chromium": [support / "Chromium"],
        }
    config = Path(os.environ.get("XDG_CONFIG_HOME") or home / ".config")
    return {
        "firefox": [
            home / ".mozilla" / "firefox",
            home / "snap" / "firefox" / "common" / ".mozilla" / "firefox",
        ],
        "chrome": [config / "google-chrome"],
        "edge": [config / "microsoft-edge"],
        "brave": [config / "BraveSoftware" / "Brave-Browser"],
        "chromium": [config / "chromium"],
    }


def local_browser_cookies_spec() -> str | None:
    """Browser whose cookies yt-dlp should read from this computer, if any.

    Only used when ClipHarbor runs on the user's own machine: a hosted deploy
    has no browser profile to read, so nothing is auto-detected there.
    """
    global _browser_cookies_spec, _browser_cookies_resolved
    if _browser_cookies_resolved:
        return _browser_cookies_spec
    _browser_cookies_resolved = True

    spec = (os.environ.get("YT_DLP_COOKIES_FROM_BROWSER") or "").strip()
    if spec:
        _browser_cookies_spec = spec
        return spec
    if (os.environ.get("CLIPHARBOR_NO_BROWSER_COOKIES") or "").strip():
        return None
    if any(os.environ.get(name) for name in CLOUD_HOST_ENV_VARS):
        return None

    for browser, dirs in _local_browser_profile_dirs().items():
        if any(directory.is_dir() for directory in dirs):
            logger.info("Using %s cookies from this computer for yt-dlp.", browser)
            _browser_cookies_spec = browser
            return browser
    return None


def yt_dlp_cookies_configured() -> bool:
    path = (os.environ.get("YT_DLP_COOKIES_FILE") or os.environ.get("YT_DLP_COOKIES_PATH") or "").strip()
    if path and Path(path).is_file():
        return True
    if (os.environ.get("YT_DLP_COOKIES_B64") or "").strip():
        return True
    return bool(local_browser_cookies_spec())


def yt_dlp_command(
    media_url: str,
    *args,
    cookies_path: str | None = None,
    browser_cookies: bool = True,
):
    cmd = [sys.executable, "-m", "yt_dlp", "--no-playlist"]
    if cookies_path:
        cmd.extend(["--cookies", cookies_path])
    elif browser_cookies:
        spec = local_browser_cookies_spec()
        if spec:
            cmd.extend(["--cookies-from-browser", spec])
    spec = (os.environ.get("YT_DLP_YOUTUBE_EXTRACTOR_ARGS") or "").strip()
    if spec and is_youtube_url(media_url):
        cmd.extend(["--extractor-args", spec])
    cmd.extend(args)
    return cmd


@contextmanager
def yt_dlp_cookies_context():
    """Netscape-format cookies for yt-dlp (needed for many YouTube URLs). See .env.example."""
    path = (os.environ.get("YT_DLP_COOKIES_FILE") or os.environ.get("YT_DLP_COOKIES_PATH") or "").strip()
    if path and Path(path).is_file():
        yield path
        return

    b64 = (os.environ.get("YT_DLP_COOKIES_B64") or "").strip()
    if not b64:
        yield None
        return

    try:
        raw = base64.b64decode(b64)
    except (ValueError, binascii.Error):
        logger.warning("YT_DLP_COOKIES_B64 is not valid base64; ignoring.")
        yield None
        return

    fd, tmp = tempfile.mkstemp(prefix="yt-cookies-", suffix=".txt")
    try:
        with os.fdopen(fd, "wb") as fh:
            fh.write(raw)
        yield tmp
    finally:
        try:
            os.unlink(tmp)
        except OSError:
            pass


def get_ffmpeg_location() -> str:
    ffmpeg_path = os.environ.get("FFMPEG_PATH")
    if ffmpeg_path:
        return ffmpeg_path
    try:
        import imageio_ffmpeg  # type: ignore

        return imageio_ffmpeg.get_ffmpeg_exe()
    except Exception:
        return "ffmpeg"


def last_error_line(stderr: str | None) -> str:
    text = (stderr or "").strip()
    if not text:
        return "The request could not be completed."
    # yt-dlp appends WARNING lines (missing JS runtime, 429s) around the real
    # exception, which buries it in API responses. Keep the ERROR lines.
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    errors = [line for line in lines if "ERROR:" in line]
    return " | ".join((errors or lines)[-2:])[:900]


COOKIE_READ_FAILURE_MARKERS = (
    "failed to decrypt",
    "app-bound",
    "could not find firefox",
    "could not find chrome",
    "unable to open database file",
    "permission denied",
)


def browser_cookies_unreadable(stderr: str | None) -> bool:
    low = (stderr or "").lower()
    return "cookies" in low and any(marker in low for marker in COOKIE_READ_FAILURE_MARKERS)


def run_yt_dlp(
    media_url: str,
    *args,
    cookies_path: str | None = None,
    timeout: int = 300,
    browser_cookies: bool = True,
):
    """Run yt-dlp, dropping browser cookies if this machine cannot supply them."""
    command = yt_dlp_command(
        media_url, *args, cookies_path=cookies_path, browser_cookies=browser_cookies
    )
    result = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    if (
        result.returncode != 0
        and not cookies_path
        and browser_cookies
        and browser_cookies_unreadable(result.stderr)
    ):
        logger.info("Could not read browser cookies; retrying without them.")
        command = yt_dlp_command(
            media_url, *args, cookies_path=None, browser_cookies=False
        )
        result = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    return result


def ytdlp_error_message(stderr: str | None, media_url: str = "") -> str:
    err = last_error_line(stderr)
    low = err.lower()
    if "sign in to confirm" not in low and "not a bot" not in low:
        return err
    parts = [err]
    if is_youtube_url(media_url):
        parts.append(
            "YouTube often blocks anonymous downloads from cloud/datacenter IPs. "
            "There is no stable free workaround that never breaks and uses nothing of yours—"
            "that is intentional on YouTube's side. "
            "To avoid putting your cookies on a server: run ClipHarbor on your home PC "
            "(residential IP); yt-dlp frequently works there with no extra setup. "
            "For cloud hosting, cookies (they expire) or similar auth are what yt-dlp supports."
        )
    elif not yt_dlp_cookies_configured():
        parts.append(
            "If this site requires a logged-in session, configure YT_DLP_COOKIES_FILE or "
            "YT_DLP_COOKIES_B64. See https://github.com/yt-dlp/yt-dlp/wiki/FAQ#how-do-i-pass-cookies-to-yt-dlp"
        )
    return " ".join(parts)


# ---------------------------------------------------------------------------
# Spotify audio lookup
#
# Spotify itself serves no downloadable audio, so the audio comes from a
# searchable source. YouTube is the usual choice, but it blocks anonymous
# downloads from datacenter IPs (Render included) with "Sign in to confirm
# you're not a bot". SoundCloud does not, so it is tried first by default.
# Change the order/selection with SPOTIFY_AUDIO_PROVIDERS.
# ---------------------------------------------------------------------------
SPOTIFY_AUDIO_SEARCH_PREFIX = {"soundcloud": "scsearch", "youtube": "ytsearch"}

# Re-uploads with these words are almost never the track the user asked for.
SPOTIFY_AUDIO_NOISE_WORDS = (
    "slowed",
    "slower",
    "sped up",
    "speed up",
    "faster",
    "reverb",
    "nightcore",
    "remix",
    "cover",
    "instrumental",
    "karaoke",
    "8d audio",
    "bass boost",
    "mashup",
    "live",
)


def spotify_audio_providers() -> list[str]:
    raw = (os.environ.get("SPOTIFY_AUDIO_PROVIDERS") or "soundcloud,youtube").strip()
    providers = [item.strip().lower() for item in raw.split(",") if item.strip()]
    known = [p for p in providers if p in SPOTIFY_AUDIO_SEARCH_PREFIX]
    return known or ["soundcloud", "youtube"]


def yt_dlp_search(query: str) -> list[dict]:
    """Run a yt-dlp search and return its result entries."""
    with yt_dlp_cookies_context() as ck:
        result = run_yt_dlp("", "--flat-playlist", "-j", query, cookies_path=ck, timeout=90)

    entries: list[dict] = []
    for line in (result.stdout or "").splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            data = json.loads(line)
        except json.JSONDecodeError:
            continue
        if data.get("webpage_url"):
            entries.append(data)

    if not entries and result.returncode != 0:
        raise RuntimeError(last_error_line(result.stderr) or last_error_line(result.stdout))
    return entries


def rank_audio_candidates(entries: list[dict], expected_duration, query: str) -> list[dict]:
    """Order the search hits by how well they match the Spotify track."""
    tokens = {t for t in re.split(r"[^a-z0-9]+", query.lower()) if len(t) > 2}
    scored: list[tuple[float, dict]] = []
    for position, entry in enumerate(entries):
        duration = entry.get("duration")
        score = position * 0.5
        if isinstance(duration, (int, float)) and duration > 0:
            if expected_duration:
                difference = abs(duration - float(expected_duration))
                # Spotify's duration and the upload rarely match exactly, but a
                # 30s preview or a different edit should never win.
                if difference > max(20.0, float(expected_duration) * 0.15):
                    continue
                score += difference
            if duration < 45 and (expected_duration or 0) > 60:
                continue
        title = (entry.get("title") or "").lower()
        title_tokens = {t for t in re.split(r"[^a-z0-9]+", title) if len(t) > 2}
        if any(word in title and word not in query.lower() for word in SPOTIFY_AUDIO_NOISE_WORDS):
            continue
        score -= 3.0 * len(tokens & title_tokens)
        # Extra words ("instrumental", "from the movie ...") mean a different audio.
        score += 2.5 * len(title_tokens - tokens)
        scored.append((score, entry))

    scored.sort(key=lambda item: item[0])
    return [entry for _, entry in scored]


def find_spotify_audio_candidates(title: str, duration) -> list[tuple[str, str]]:
    """Return ranked (media_url, provider) options for a Spotify track title."""
    candidates: list[tuple[str, str]] = []
    problems: list[str] = []
    for provider in spotify_audio_providers():
        prefix = SPOTIFY_AUDIO_SEARCH_PREFIX[provider]
        try:
            entries = yt_dlp_search(f"{prefix}6:{title}")
        except subprocess.TimeoutExpired:
            problems.append(f"{provider}: tempo esgotado na busca")
            continue
        except Exception as exc:
            problems.append(f"{provider}: {exc}")
            continue

        ranked = rank_audio_candidates(entries, duration, title)
        if not ranked:
            problems.append(f"{provider}: nenhum resultado compatível")
            continue
        candidates.extend((entry["webpage_url"], provider) for entry in ranked[:2])

    if not candidates:
        detail = " | ".join(problems) or "nenhum provedor configurado"
        raise RuntimeError(f"Não encontrei o áudio desta faixa do Spotify ({detail}).")
    return candidates[:3]


def download_audio_file(media_url: str, output_template: str, ffmpeg_path: str) -> str:
    """Download media_url as MP3 and return the produced file path."""
    with yt_dlp_cookies_context() as ck:
        result = run_yt_dlp(
            media_url,
            "-x",
            "--audio-format",
            "mp3",
            "--audio-quality",
            "0",
            "--no-progress",
            "-o",
            output_template,
            "--ffmpeg-location",
            ffmpeg_path,
            media_url,
            cookies_path=ck,
            timeout=300,
        )

    if result.returncode != 0:
        raise RuntimeError(last_error_line(result.stderr) or last_error_line(result.stdout))

    produced = sorted(glob.glob(str(Path(output_template).parent / "*.mp3")))
    if not produced:
        raise RuntimeError("o download terminou sem gerar um MP3")
    return produced[0]


def fetch_cover_art(cover_url: str, destination_dir: Path) -> Path | None:
    if not cover_url:
        return None

    try:
        req = urllib.request.Request(cover_url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=20) as resp:
            data = resp.read()
        cover_path = destination_dir / "cover.jpg"
        cover_path.write_bytes(data)
        return cover_path if data else None
    except Exception as exc:
        logger.warning("Could not fetch Spotify cover art %s: %s", cover_url, exc)
        return None


def split_spotify_title(title: str) -> tuple[str, str]:
    """Split the "Artist - Track" convention into (artist, track)."""
    if " - " in title:
        artist, name = title.split(" - ", 1)
        return artist.strip(), name.strip()
    return "", title.strip()


def tag_spotify_audio(audio_path: str, title: str, album: str, cover_url: str, ffmpeg_path: str) -> str:
    """Write Spotify tags and artwork into the downloaded MP3 (ffmpeg only, no ffprobe)."""
    source = Path(audio_path)
    artist, name = split_spotify_title(title)
    cover_path = fetch_cover_art(cover_url, source.parent)
    tagged = source.with_name(f"{sanitize_filename(title, 'spotify-track')}.mp3")

    command = [ffmpeg_path, "-y", "-loglevel", "error", "-i", str(source)]
    if cover_path:
        command += [
            "-i",
            str(cover_path),
            "-map",
            "0:a",
            "-map",
            "1:v",
            "-c:v",
            "mjpeg",
            "-id3v2_version",
            "3",
            "-metadata:s:v",
            "title=Album cover",
            "-metadata:s:v",
            "comment=Cover (front)",
        ]
    else:
        command += ["-map", "0:a"]
    command += ["-c:a", "copy", "-metadata", f"title={name}"]
    if artist:
        command += ["-metadata", f"artist={artist}"]
    if album:
        command += ["-metadata", f"album={album}"]
    command.append(str(tagged))

    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=120)
    except subprocess.TimeoutExpired:
        logger.warning("Timed out tagging %s; keeping the untagged file", source.name)
        return audio_path

    if result.returncode != 0 or not tagged.is_file():
        logger.warning("ffmpeg could not tag %s: %s", source.name, last_error_line(result.stderr))
        return audio_path

    try:
        source.unlink()
    except OSError:
        pass
    return str(tagged)


def download_spotify_track(
    title: str, duration, cover_url: str, album: str, track_dir: Path, ffmpeg_path: str
) -> str:
    """Resolve a Spotify track to an audio source, download it and tag the MP3."""
    track_dir.mkdir(parents=True, exist_ok=True)
    failures: list[str] = []
    for media_url, provider in find_spotify_audio_candidates(title, duration):
        logger.info("Spotify track %r resolved via %s: %s", title, provider, media_url)
        try:
            audio_path = download_audio_file(
                media_url, str(track_dir / "source.%(ext)s"), ffmpeg_path
            )
        except subprocess.TimeoutExpired:
            raise
        except Exception as exc:
            # A single hit can be DRM protected or region locked; try the next one.
            logger.warning("Audio source %s failed for %r: %s", media_url, title, exc)
            failures.append(f"{provider}: {exc}")
            for leftover in track_dir.glob("source.*"):
                try:
                    leftover.unlink()
                except OSError:
                    pass
            continue
        return tag_spotify_audio(audio_path, title, album, cover_url, ffmpeg_path)

    detail = " | ".join(failures) or "nenhuma fonte de áudio disponível"
    raise RuntimeError(f"Nenhuma fonte de áudio funcionou para esta faixa ({detail}).")


def spotify_download_targets(url: str) -> tuple[list[dict], str | None]:
    """Return ([{title, duration, cover, album}], playlist_title)."""
    info = get_spotify_info(url)
    if info.get("is_playlist"):
        tracks = info.get("tracks") or []
        if not tracks:
            raise RuntimeError(
                "Não consegui listar as faixas deste playlist/álbum do Spotify. Tente o link de uma faixa."
            )
        album = info.get("title") or ""
        targets = [
            {
                "title": track.get("title") or "",
                "duration": track.get("duration"),
                "cover": track.get("thumbnail") or "",
                "album": album,
            }
            for track in tracks
        ]
        return targets, info.get("title")

    return (
        [
            {
                "title": info.get("title") or "",
                "duration": info.get("duration"),
                "cover": info.get("thumbnail") or "",
                "album": "",
            }
        ],
        None,
    )


def sanitize_filename(value: str, fallback: str) -> str:
    cleaned = "".join(ch for ch in value if ch not in r'\/:*?"<>|').strip()
    cleaned = " ".join(cleaned.split())[:80].strip(" .")
    return cleaned or fallback


def is_vercel() -> bool:
    return os.environ.get("VERCEL") == "1"


def blob_configured() -> bool:
    return vercel_blob is not None and bool(os.environ.get("BLOB_READ_WRITE_TOKEN"))


def upload_to_blob_and_respond(chosen_file: str, download_filename: str, temp_dir: Path):
    """Upload finished media to Vercel Blob and return a small JSON payload (avoids the ~4.5MB function response limit)."""
    if vercel_blob is None:
        shutil.rmtree(temp_dir, ignore_errors=True)
        return jsonify({"error": "Vercel Blob SDK is unavailable in this deployment."}), 503
    path = Path(chosen_file)
    blob_path = f"clipharbor/{uuid.uuid4().hex}/{path.name}"
    try:
        with open(chosen_file, "rb") as fh:
            data = fh.read()
    except OSError as exc:
        logger.exception("Could not read output file for upload")
        shutil.rmtree(temp_dir, ignore_errors=True)
        return jsonify({"error": str(exc)}), 500

    large = len(data) > 6 * 1024 * 1024
    timeout = 300 if large else 120
    try:
        meta = vercel_blob.put(
            blob_path,
            data,
            {"addRandomSuffix": "true"},
            timeout=timeout,
            multipart=large,
        )
    except Exception as exc:
        logger.exception("Vercel Blob upload failed")
        shutil.rmtree(temp_dir, ignore_errors=True)
        return jsonify({"error": f"Could not upload file: {exc}"}), 500

    shutil.rmtree(temp_dir, ignore_errors=True)
    file_url = meta.get("downloadUrl") or meta.get("url")
    if not file_url:
        return jsonify({"error": "Storage did not return a download URL."}), 500
    return jsonify({"download_url": file_url, "filename": download_filename})


def pick_download_file(files: list[str], format_choice: str) -> str:
    preferred_extension = ".mp3" if format_choice == "audio" else ".mp4"
    matches = [p for p in files if p.endswith(preferred_extension)]
    if matches:
        return matches[0]
    # Log fallback so we know when the preferred format wasn't produced
    logger.warning(
        "Preferred extension %s not found among %s; falling back to %s",
        preferred_extension,
        files,
        files[0],
    )
    return files[0]


# ---------------------------------------------------------------------------
# Routes
# ---------------------------------------------------------------------------
@app.route("/")
def index():
    return render_template("index.html", app_name=APP_NAME)


@app.route("/favicon.svg")
def favicon():
    return send_from_directory(PUBLIC_DIR, "favicon.svg")


@app.route("/api/info", methods=["POST"])
@limiter.limit("30 per hour")
def get_info():
    payload = request.get_json(silent=True) or {}
    url = payload.get("url", "").strip()

    if not url:
        return jsonify({"error": "No URL provided"}), 400

    if not url.startswith(("http://", "https://")):
        return jsonify({"error": "Invalid URL"}), 400

    if is_spotify_url(url):
        try:
            info = get_spotify_info(url)
            return jsonify(info)
        except Exception as exc:
            logger.exception("Failed to get Spotify info for %s", url)
            return jsonify({"error": f"Erro ao buscar detalhes do Spotify: {exc}"}), 400

    logger.info("Fetching info for: %s", url)

    try:
        with yt_dlp_cookies_context() as ck:
            result = run_yt_dlp(url, "-j", url, cookies_path=ck, timeout=60)
        if result.returncode != 0:
            err = ytdlp_error_message(result.stderr, url)
            logger.warning("yt-dlp info failed for %s: %s", url, err)
            return jsonify({"error": err}), 400

        info = json.loads(result.stdout)

        best_by_height: dict = {}
        for item in info.get("formats", []):
            height = item.get("height")
            if height and item.get("vcodec", "none") != "none":
                bitrate = item.get("tbr") or 0
                if height not in best_by_height or bitrate > (best_by_height[height].get("tbr") or 0):
                    best_by_height[height] = item

        formats = [
            {"id": item["format_id"], "label": f"{height}p", "height": height}
            for height, item in best_by_height.items()
        ]
        formats.sort(key=lambda f: f["height"], reverse=True)

        return jsonify(
            {
                "title": info.get("title", ""),
                "thumbnail": info.get("thumbnail", ""),
                "duration": info.get("duration"),
                "uploader": info.get("uploader", ""),
                "formats": formats,
            }
        )

    except subprocess.TimeoutExpired:
        logger.error("Timeout fetching info for: %s", url)
        return jsonify({"error": "Timed out while fetching media details."}), 400
    except Exception as exc:
        logger.exception("Unexpected error in /api/info for %s", url)
        return jsonify({"error": str(exc)}), 400


@app.route("/api/download", methods=["POST"])
@limiter.limit("20 per hour")
def download_media():
    payload = request.get_json(silent=True) or {}
    url = payload.get("url", "").strip()
    format_choice = payload.get("format", "video")
    format_id = payload.get("format_id")
    title = payload.get("title", "")

    if not url:
        return jsonify({"error": "No URL provided"}), 400

    if not url.startswith(("http://", "https://")):
        return jsonify({"error": "Invalid URL"}), 400

    if is_spotify_url(url):
        temp_dir = Path(tempfile.mkdtemp(prefix="clipharbor-spotify-"))
        ffmpeg_path = get_ffmpeg_location()
        logger.info("Downloading Spotify track %s into %s", url, temp_dir)
        try:
            started_at = time.monotonic()
            targets, playlist_title = spotify_download_targets(url)
            logger.info("Spotify %s resolved to %d track(s)", url, len(targets))

            files: list[str] = []
            failed_tracks: list[str] = []
            for index, target in enumerate(targets, start=1):
                track_title = target["title"]
                logger.info("Spotify track %d/%d: %s", index, len(targets), track_title)
                try:
                    files.append(
                        download_spotify_track(
                            track_title,
                            target.get("duration"),
                            target.get("cover") or "",
                            target.get("album") or "",
                            temp_dir / f"track-{index:02d}",
                            ffmpeg_path,
                        )
                    )
                except subprocess.TimeoutExpired:
                    raise
                except Exception as exc:
                    logger.warning("Spotify track %r failed: %s", track_title, exc)
                    if len(targets) == 1:
                        raise RuntimeError(f"Erro no download do Spotify: {exc}") from exc
                    # Playlists keep going: one unavailable song should not
                    # throw away the tracks that did download.
                    failed_tracks.append(f"{index}. {track_title} - {exc}")

            logger.info("Spotify download finished for %s in %.1fs", url, time.monotonic() - started_at)

            if not files:
                raise RuntimeError("Nenhuma faixa deste playlist/álbum pôde ser baixada.")

            if len(files) > 1 or is_spotify_playlist_url(url):
                zip_base = sanitize_filename(title or playlist_title or "spotify_playlist", "playlist")
                zip_filename = f"{zip_base}.zip"
                zip_filepath = str(temp_dir / zip_filename)
                with zipfile.ZipFile(zip_filepath, "w", zipfile.ZIP_DEFLATED) as zf:
                    for f in files:
                        if f != zip_filepath:
                            zf.write(f, arcname=Path(f).name)
                    if failed_tracks:
                        report = temp_dir / "faixas-nao-baixadas.txt"
                        report.write_text("\n".join(failed_tracks), encoding="utf-8")
                        zf.write(report, arcname=report.name)
                chosen_file = zip_filepath
                filename = zip_filename
            else:
                chosen_file = files[0]
                filename = Path(chosen_file).name

            logger.info("Serving Spotify file: %s as %s", chosen_file, filename)

            if blob_configured():
                return upload_to_blob_and_respond(chosen_file, filename, temp_dir)

            if is_vercel():
                shutil.rmtree(temp_dir, ignore_errors=True)
                return (
                    jsonify(
                        {
                            "error": (
                                "Large downloads on Vercel need Blob storage. In the Vercel dashboard: "
                                "Storage → Create Blob → connect it to this project, then redeploy so "
                                "BLOB_READ_WRITE_TOKEN is set."
                            )
                        }
                    ),
                    503,
                )

            response = send_file(
                chosen_file,
                as_attachment=True,
                download_name=filename,
                max_age=0,
            )
            response.call_on_close(lambda: shutil.rmtree(temp_dir, ignore_errors=True))
            return response
        except subprocess.TimeoutExpired:
            logger.error("Timeout downloading Spotify URL: %s", url)
            shutil.rmtree(temp_dir, ignore_errors=True)
            return jsonify({"error": "Download do Spotify expirou (tempo limite excedido)."}), 400
        except RuntimeError as exc:
            logger.warning("Spotify download failed for %s: %s", url, exc)
            shutil.rmtree(temp_dir, ignore_errors=True)
            return jsonify({"error": str(exc)}), 400
        except Exception as exc:
            logger.exception("Unexpected error downloading Spotify URL: %s", url)
            shutil.rmtree(temp_dir, ignore_errors=True)
            return jsonify({"error": str(exc)}), 500

    if format_choice not in ("video", "audio"):
        return jsonify({"error": "Invalid format"}), 400

    temp_dir = Path(tempfile.mkdtemp(prefix="clipharbor-"))
    output_template = str(temp_dir / "clipharbor.%(ext)s")
    ffmpeg_path = get_ffmpeg_location()

    logger.info("Downloading [%s] %s (format_id=%s)", format_choice, url, format_id)

    try:
        with yt_dlp_cookies_context() as ck:
            extra_args = ["-o", output_template]
            if format_choice == "audio":
                extra_args += ["-x", "--audio-format", "mp3", "--ffmpeg-location", ffmpeg_path]
            elif format_id:
                extra_args += [
                    "-f", f"{format_id}+bestaudio/best",
                    "--merge-output-format", "mp4",
                    "--ffmpeg-location", ffmpeg_path,
                ]
            else:
                extra_args += [
                    "-f", "bestvideo+bestaudio/best",
                    "--merge-output-format", "mp4",
                    "--ffmpeg-location", ffmpeg_path,
                ]
            extra_args.append(url)
            result = run_yt_dlp(url, *extra_args, cookies_path=ck, timeout=300)

        if result.returncode != 0:
            err = ytdlp_error_message(result.stderr, url)
            logger.warning("yt-dlp download failed for %s: %s", url, err)
            shutil.rmtree(temp_dir, ignore_errors=True)
            return jsonify({"error": err}), 400

        files = glob.glob(str(temp_dir / "clipharbor.*"))
        if not files:
            logger.error("No output file produced for: %s", url)
            shutil.rmtree(temp_dir, ignore_errors=True)
            return jsonify({"error": "Download finished, but no file was created."}), 500

        chosen_file = pick_download_file(files, format_choice)
        extension = Path(chosen_file).suffix
        fallback_name = f"{APP_NAME.lower()}{extension}"
        filename = f"{sanitize_filename(title, APP_NAME)}{extension}" if title else fallback_name

        logger.info("Serving file: %s as %s", chosen_file, filename)

        if blob_configured():
            return upload_to_blob_and_respond(chosen_file, filename, temp_dir)

        if is_vercel():
            shutil.rmtree(temp_dir, ignore_errors=True)
            return (
                jsonify(
                    {
                        "error": (
                            "Large downloads on Vercel need Blob storage. In the Vercel dashboard: "
                            "Storage → Create Blob → connect it to this project, then redeploy so "
                            "BLOB_READ_WRITE_TOKEN is set."
                        )
                    }
                ),
                503,
            )

        response = send_file(
            chosen_file,
            as_attachment=True,
            download_name=filename,
            max_age=0,
        )
        response.call_on_close(lambda: shutil.rmtree(temp_dir, ignore_errors=True))
        return response

    except subprocess.TimeoutExpired:
        logger.error("Download timed out for: %s", url)
        shutil.rmtree(temp_dir, ignore_errors=True)
        return jsonify({"error": "Download timed out after 5 minutes."}), 400
    except Exception as exc:
        logger.exception("Unexpected error in /api/download for %s", url)
        shutil.rmtree(temp_dir, ignore_errors=True)
        return jsonify({"error": str(exc)}), 500


# ---------------------------------------------------------------------------
# Rate limit error handler
# ---------------------------------------------------------------------------
@app.errorhandler(429)
def ratelimit_handler(e):
    return jsonify({"error": "Too many requests. Please slow down."}), 429


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------
if __name__ == "__main__":
    port = int(os.environ.get("PORT", 8899))
    host = os.environ.get("HOST", "0.0.0.0")
    debug = os.environ.get("FLASK_DEBUG", "false").lower() == "true"
    app.run(host=host, port=port, debug=debug)
