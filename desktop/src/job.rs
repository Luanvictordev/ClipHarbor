//! Download pipeline: Spotify links (through search) and direct media links.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::deps;
use crate::spotify::{self, Track};
use crate::tags;
use crate::ytdlp::{self, YtDlp};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Audio,
    Video,
}

pub enum Event {
    Log(String),
    Progress(f32),
    Finished(Outcome),
    Failed(String),
}

#[derive(Default)]
pub struct Outcome {
    pub files: Vec<PathBuf>,
    pub failures: Vec<String>,
}

/// Run a download on the calling thread, reporting through `tx`.
pub fn run(
    url: String,
    format: Format,
    cookies: Option<String>,
    out_dir: PathBuf,
    tx: Sender<Event>,
    notify: impl Fn() + Send + 'static,
) {
    let event = match execute(&url, format, cookies, &out_dir, &tx, &notify) {
        Ok(outcome) => Event::Finished(outcome),
        Err(err) => Event::Failed(err),
    };
    let _ = tx.send(event);
    notify();
}

fn log(tx: &Sender<Event>, notify: &impl Fn(), message: impl Into<String>) {
    let _ = tx.send(Event::Log(message.into()));
    notify();
}

fn execute(
    url: &str,
    format: Format,
    cookies: Option<String>,
    out_dir: &Path,
    tx: &Sender<Event>,
    notify: &impl Fn(),
) -> Result<Outcome, String> {
    std::fs::create_dir_all(out_dir)
        .map_err(|err| format!("Não consegui criar a pasta de destino: {err}"))?;

    log(tx, notify, "Procurando yt-dlp e ffmpeg...");
    let exe = deps::ensure_ytdlp(|line| log(tx, notify, line))?;
    let ffmpeg = deps::ensure_ffmpeg(|line| log(tx, notify, line))?;

    let ytdlp = YtDlp {
        exe,
        ffmpeg,
        cookies: cookies.filter(|value| !value.trim().is_empty()),
    };

    if spotify::is_spotify_url(url) {
        return download_spotify(&ytdlp, url, out_dir, tx, notify);
    }

    if format == Format::Video {
        return download_direct_video(&ytdlp, url, out_dir, tx, notify);
    }
    download_direct_audio(&ytdlp, url, out_dir, tx, notify)
}

fn temporary_dir() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("clipharbor-{stamp}"))
}

fn newest_file(dir: &Path, extensions: &[&str]) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !extensions.iter().any(|wanted| *wanted == extension) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if best.as_ref().map(|(when, _)| modified > *when).unwrap_or(true) {
            best = Some((modified, path));
        }
    }
    best.map(|(_, path)| path)
}

/// Characters Windows does not allow in a file name.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if (c as u32) < 0x20 => ' ',
            c => c,
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_matches(['.', ' ']).trim();
    if trimmed.is_empty() {
        "faixa".to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}

fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|err| format!("não consegui salvar o arquivo: {err}"))
}

fn fetch_cover(url: &str, dir: &Path) -> Option<PathBuf> {
    if url.is_empty() {
        return None;
    }
    let destination = dir.join("cover.jpg");
    deps::curl_download(url, &destination).ok().map(|_| destination)
}

fn download_spotify(
    ytdlp: &YtDlp,
    url: &str,
    out_dir: &Path,
    tx: &Sender<Event>,
    notify: &impl Fn(),
) -> Result<Outcome, String> {
    log(tx, notify, "Lendo os metadados do Spotify...");
    let entity = spotify::fetch(url)?;
    let total = entity.tracks.len();
    log(
        tx,
        notify,
        format!("{} — {total} faixa(s)", entity.name),
    );

    let temp = temporary_dir();
    std::fs::create_dir_all(&temp).map_err(|err| format!("{err}"))?;

    let mut outcome = Outcome::default();
    for (index, track) in entity.tracks.iter().enumerate() {
        log(
            tx,
            notify,
            format!("[{}/{}] {}", index + 1, total, track.title),
        );
        match download_track(ytdlp, track, out_dir, &temp, tx, notify) {
            Ok(path) => {
                log(
                    tx,
                    notify,
                    format!("    salvo: {}", path.file_name().unwrap_or_default().to_string_lossy()),
                );
                outcome.files.push(path);
            }
            Err(err) => {
                log(tx, notify, format!("    falhou: {err}"));
                outcome.failures.push(format!("{} — {err}", track.title));
            }
        }
        let _ = tx.send(Event::Progress((index + 1) as f32 / total as f32));
        notify();
    }

    let _ = std::fs::remove_dir_all(&temp);

    if !outcome.failures.is_empty() {
        let report = out_dir.join("faixas-nao-baixadas.txt");
        let body = format!(
            "Faixas sem fonte de áudio ({}/{}):\n\n{}",
            outcome.failures.len(),
            total,
            outcome.failures.join("\n")
        );
        let _ = std::fs::write(&report, body);
        log(
            tx,
            notify,
            format!("{} faixa(s) sem fonte. Lista em {}", outcome.failures.len(), report.display()),
        );
    }

    if outcome.files.is_empty() {
        return Err("Nenhuma faixa pôde ser baixada.".to_string());
    }
    Ok(outcome)
}

fn download_track(
    ytdlp: &YtDlp,
    track: &Track,
    out_dir: &Path,
    temp: &Path,
    tx: &Sender<Event>,
    notify: &impl Fn(),
) -> Result<PathBuf, String> {
    let candidates = ytdlp::find_candidates(ytdlp, &track.title, track.duration)?;
    let mut last_error = String::from("nenhum candidato");

    for (index, candidate) in candidates.iter().enumerate() {
        let template = temp.join(format!("track-{index}.%(ext)s"));
        match ytdlp.download_audio(&ytdlp::media_url(candidate), &template) {
            Ok(()) => {
                let produced = newest_file(temp, &["mp3"])
                    .ok_or_else(|| "o download terminou sem gerar um MP3".to_string())?;
                let cover = fetch_cover(&track.cover, temp);
                let _ = tags::tag_mp3(
                    &produced,
                    &track.name,
                    &track.artist,
                    &track.album,
                    cover.as_deref(),
                );
                let destination = out_dir.join(format!("{}.mp3", sanitize(&track.title)));
                move_file(&produced, &destination)?;
                return Ok(destination);
            }
            Err(err) => {
                log(tx, notify, format!("    {}: {err}", short_source(&ytdlp::media_url(candidate))));
                last_error = err;
            }
        }
    }
    Err(last_error)
}

fn short_source(url: &str) -> &'static str {
    if url.contains("youtube.com") || url.contains("youtu.be") {
        "youtube"
    } else if url.contains("soundcloud.com") {
        "soundcloud"
    } else {
        "fonte"
    }
}

fn download_direct_audio(
    ytdlp: &YtDlp,
    url: &str,
    out_dir: &Path,
    tx: &Sender<Event>,
    notify: &impl Fn(),
) -> Result<Outcome, String> {
    log(tx, notify, "Baixando o áudio...");
    let temp = temporary_dir();
    std::fs::create_dir_all(&temp).map_err(|err| format!("{err}"))?;

    let template = temp.join("audio.%(ext)s");
    ytdlp.download_audio(url, &template)?;
    let produced = newest_file(&temp, &["mp3"])
        .ok_or_else(|| "o download terminou sem gerar um MP3".to_string())?;
    let destination = out_dir.join(format!("{}.mp3", sanitize("audio")));
    move_file(&produced, &destination)?;
    let _ = std::fs::remove_dir_all(&temp);
    let _ = tx.send(Event::Progress(1.0));
    notify();
    Ok(Outcome {
        files: vec![destination],
        failures: Vec::new(),
    })
}

fn download_direct_video(
    ytdlp: &YtDlp,
    url: &str,
    out_dir: &Path,
    tx: &Sender<Event>,
    notify: &impl Fn(),
) -> Result<Outcome, String> {
    log(tx, notify, "Baixando o vídeo...");
    let temp = temporary_dir();
    std::fs::create_dir_all(&temp).map_err(|err| format!("{err}"))?;

    let template = temp.join("video.%(ext)s");
    ytdlp.download_video(url, &template)?;
    let produced = newest_file(&temp, &["mp4", "mkv", "webm"])
        .ok_or_else(|| "o download terminou sem gerar um vídeo".to_string())?;
    let extension = produced
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4")
        .to_string();
    let destination = out_dir.join(format!("{}.{extension}", sanitize("video")));
    move_file(&produced, &destination)?;
    let _ = std::fs::remove_dir_all(&temp);
    let _ = tx.send(Event::Progress(1.0));
    notify();
    Ok(Outcome {
        files: vec![destination],
        failures: Vec::new(),
    })
}
