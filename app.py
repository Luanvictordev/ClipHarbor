import base64
import binascii
import glob
import json
import logging
import os
import shutil
import subprocess
import sys
import tempfile
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
except ImportError:
    try:
        from vercel import blob as vercel_blob
    except ImportError:
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


_spotify_client_initialized = False


def ensure_spotify_client():
    global _spotify_client_initialized
    if not _spotify_client_initialized:
        try:
            from spotdl.utils.spotify import SpotifyClient

            SpotifyClient.init("", "")
            _spotify_client_initialized = True
        except Exception as exc:
            logger.warning("Could not initialize SpotifyClient: %s", exc)


def get_spotify_track_info(url: str) -> dict:
    ensure_spotify_client()
    try:
        from spotdl.types.song import Song

        song = Song.from_url(url)
        artists = ", ".join(song.artists) if song.artists else ""
        title = f"{artists} - {song.name}" if artists else song.name
        return {
            "title": title,
            "thumbnail": song.cover_url or "",
            "duration": song.duration,
            "uploader": artists or "Spotify",
            "formats": [
                {"id": "mp3", "label": "MP3 Audio (320kbps)", "height": 0}
            ],
        }
    except Exception as exc:
        logger.warning("spotdl metadata failed for %s (%s); trying oEmbed fallback", url, exc)
        import urllib.request

        req = urllib.request.Request(
            f"https://open.spotify.com/oembed?url={url}",
            headers={"User-Agent": "Mozilla/5.0"},
        )
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode())
        title = data.get("title") or "Spotify Track"
        thumb = data.get("thumbnail_url") or ""
        return {
            "title": title,
            "thumbnail": thumb,
            "duration": None,
            "uploader": "Spotify",
            "formats": [
                {"id": "mp3", "label": "MP3 Audio (320kbps)", "height": 0}
            ],
        }


def get_spotify_info(url: str) -> dict:
    ensure_spotify_client()
    if is_spotify_playlist_url(url):
        try:
            if "/album/" in url.lower():
                from spotdl.types.album import Album

                album = Album.from_url(url, fetch_songs=False)
                songs = album.songs
                name = album.name
            else:
                from spotdl.types.playlist import Playlist

                playlist = Playlist.from_url(url, fetch_songs=False)
                songs = playlist.songs
                name = playlist.name

            tracks = []
            for s in songs:
                artists = ", ".join(s.artists) if s.artists else ""
                title = f"{artists} - {s.name}" if artists else s.name
                tracks.append(
                    {
                        "url": s.url,
                        "title": title,
                        "thumbnail": s.cover_url or "",
                        "duration": s.duration,
                        "uploader": artists or "Spotify",
                        "formats": [
                            {"id": "mp3", "label": "MP3 Audio (320kbps)", "height": 0}
                        ],
                    }
                )

            return {
                "is_playlist": True,
                "title": name,
                "track_count": len(tracks),
                "tracks": tracks,
            }
        except Exception as exc:
            logger.warning("spotdl playlist lookup failed for %s (%s); trying oEmbed fallback", url, exc)
            import urllib.request

            req = urllib.request.Request(
                f"https://open.spotify.com/oembed?url={url}",
                headers={"User-Agent": "Mozilla/5.0"},
            )
            with urllib.request.urlopen(req, timeout=10) as resp:
                data = json.loads(resp.read().decode())
            return {
                "is_playlist": True,
                "title": data.get("title") or "Spotify Playlist",
                "track_count": 0,
                "tracks": [],
                "thumbnail": data.get("thumbnail_url") or "",
            }

    return get_spotify_track_info(url)


def yt_dlp_cookies_configured() -> bool:
    path = (os.environ.get("YT_DLP_COOKIES_FILE") or os.environ.get("YT_DLP_COOKIES_PATH") or "").strip()
    if path and Path(path).is_file():
        return True
    return bool((os.environ.get("YT_DLP_COOKIES_B64") or "").strip())


def yt_dlp_command(media_url: str, *args, cookies_path: str | None = None):
    cmd = [sys.executable, "-m", "yt_dlp", "--no-playlist"]
    if cookies_path:
        cmd.extend(["--cookies", cookies_path])
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
    return text.splitlines()[-1]


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
            command = yt_dlp_command(url, "-j", url, cookies_path=ck)
            result = subprocess.run(command, capture_output=True, text=True, timeout=60)
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
            cmd = [
                sys.executable,
                "-m",
                "spotdl",
                "download",
                url,
                "--output",
                str(temp_dir),
                "--ffmpeg",
                ffmpeg_path,
                "--format",
                "mp3",
                "--threads",
                "4",
            ]
            result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
            if result.returncode != 0:
                err = last_error_line(result.stderr) or last_error_line(result.stdout)
                logger.warning("spotdl download failed for %s: %s", url, err)
                shutil.rmtree(temp_dir, ignore_errors=True)
                return jsonify({"error": f"Erro no download do Spotify: {err}"}), 400

            files = glob.glob(str(temp_dir / "*.mp3"))
            if not files:
                logger.error("No MP3 file produced for Spotify URL: %s", url)
                shutil.rmtree(temp_dir, ignore_errors=True)
                return jsonify({"error": "O download concluiu, mas nenhum arquivo MP3 foi gerado."}), 500

            if len(files) > 1 or is_spotify_playlist_url(url):
                zip_base = sanitize_filename(title or "spotify_playlist", "playlist")
                zip_filename = f"{zip_base}.zip"
                zip_filepath = str(temp_dir / zip_filename)
                with zipfile.ZipFile(zip_filepath, "w", zipfile.ZIP_DEFLATED) as zf:
                    for f in files:
                        if f != zip_filepath:
                            zf.write(f, arcname=Path(f).name)
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
            command = yt_dlp_command(url, "-o", output_template, cookies_path=ck)
            if format_choice == "audio":
                command += ["-x", "--audio-format", "mp3", "--ffmpeg-location", ffmpeg_path]
            elif format_id:
                command += [
                    "-f", f"{format_id}+bestaudio/best",
                    "--merge-output-format", "mp4",
                    "--ffmpeg-location", ffmpeg_path,
                ]
            else:
                command += [
                    "-f", "bestvideo+bestaudio/best",
                    "--merge-output-format", "mp4",
                    "--ffmpeg-location", ffmpeg_path,
                ]
            command.append(url)
            result = subprocess.run(command, capture_output=True, text=True, timeout=300)

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
