//! Download pipeline: Spotify links (through search) and direct media links.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::deps;
use crate::spotify::{self, Track};
use crate::tags;
use crate::ytdlp::{self, YtDlp};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Audio,
    Video,
}

/// Severity of a log line, so the window can colour it instead of guessing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Ok,
    Warn,
    Error,
}

/// Everything one download needs, shared by the window and the `--cli` mode.
pub struct Request {
    pub url: String,
    pub format: Format,
    /// Browser whose cookies yt-dlp should read, e.g. "firefox".
    pub browser: Option<String>,
    /// Netscape `cookies.txt` for browsers yt-dlp cannot read on its own.
    pub cookies_file: Option<PathBuf>,
    pub out_dir: PathBuf,
}

pub enum Event {
    Log(Level, String),
    /// A track is starting; `index` is one-based.
    Track {
        index: usize,
        total: usize,
        title: String,
    },
    Progress(f32),
    Finished(Outcome),
    Failed(String),
}

#[derive(Default)]
pub struct Outcome {
    pub files: Vec<PathBuf>,
    pub failures: Vec<String>,
    pub cancelled: bool,
}

/// Run a download on the calling thread, reporting through `tx`.
pub fn run(
    request: Request,
    cancel: Arc<AtomicBool>,
    tx: Sender<Event>,
    notify: impl Fn() + Send + 'static,
) {
    let event = match execute(&request, &cancel, &tx, &notify) {
        Ok(outcome) => Event::Finished(outcome),
        Err(err) => Event::Failed(err),
    };
    let _ = tx.send(event);
    notify();
}

fn emit(tx: &Sender<Event>, notify: &impl Fn(), level: Level, message: impl Into<String>) {
    let _ = tx.send(Event::Log(level, message.into()));
    notify();
}

fn log(tx: &Sender<Event>, notify: &impl Fn(), message: impl Into<String>) {
    emit(tx, notify, Level::Info, message);
}

fn stopped(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

fn execute(
    request: &Request,
    cancel: &Arc<AtomicBool>,
    tx: &Sender<Event>,
    notify: &impl Fn(),
) -> Result<Outcome, String> {
    std::fs::create_dir_all(&request.out_dir)
        .map_err(|err| format!("Não consegui criar a pasta de destino: {err}"))?;

    log(tx, notify, "Preparando yt-dlp e ffmpeg...");
    let exe = deps::ensure_ytdlp(|line| log(tx, notify, line))?;
    let ffmpeg = deps::ensure_ffmpeg(|line| log(tx, notify, line))?;

    let mut browser = request
        .browser
        .clone()
        .filter(|value| !value.trim().is_empty() && value != "nenhum");
    if let Some(name) = browser.as_deref() {
        if !deps::installed_browsers().contains(&name) {
            emit(
                tx,
                notify,
                Level::Warn,
                format!("Não encontrei um perfil do {name} neste PC; seguindo sem cookies."),
            );
            browser = None;
        }
    }

    let cookies_file = request.cookies_file.clone().filter(|path| {
        if path.is_file() {
            true
        } else {
            emit(
                tx,
                notify,
                Level::Warn,
                format!(
                    "O arquivo de cookies não existe ({}); seguindo sem ele.",
                    path.display()
                ),
            );
            false
        }
    });

    let ytdlp = YtDlp::new(exe, ffmpeg, browser, cookies_file);

    if spotify::is_spotify_url(&request.url) {
        return download_spotify(&ytdlp, request, cancel, tx, notify);
    }

    if stopped(cancel) {
        return Ok(Outcome {
            cancelled: true,
            ..Outcome::default()
        });
    }

    if request.format == Format::Video {
        return download_direct_video(&ytdlp, request, tx, notify);
    }
    download_direct_audio(&ytdlp, request, tx, notify)
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
        if best
            .as_ref()
            .map(|(when, _)| modified > *when)
            .unwrap_or(true)
        {
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

fn fetch_cover(url: &str, dir: &Path) -> Result<Option<PathBuf>, String> {
    if url.is_empty() {
        return Ok(None);
    }
    let destination = dir.join("cover.jpg");
    deps::curl_download(url, &destination)
        .map(|_| Some(destination))
        .map_err(|err| format!("não consegui baixar a capa: {err}"))
}

fn download_spotify(
    ytdlp: &YtDlp,
    request: &Request,
    cancel: &AtomicBool,
    tx: &Sender<Event>,
    notify: &impl Fn(),
) -> Result<Outcome, String> {
    log(tx, notify, "Lendo os metadados do Spotify...");
    let entity = spotify::fetch(&request.url)?;
    let total = entity.tracks.len();
    log(tx, notify, format!("{} — {total} faixa(s)", entity.name));

    let temp = temporary_dir();
    std::fs::create_dir_all(&temp).map_err(|err| format!("{err}"))?;

    let mut outcome = Outcome::default();
    for (index, track) in entity.tracks.iter().enumerate() {
        if stopped(cancel) {
            outcome.cancelled = true;
            break;
        }

        let _ = tx.send(Event::Track {
            index: index + 1,
            total,
            title: track.title.clone(),
        });
        notify();

        match download_track(ytdlp, track, &request.out_dir, &temp, tx, notify) {
            Ok(path) => {
                emit(
                    tx,
                    notify,
                    Level::Ok,
                    format!(
                        "{} salvo",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                );
                outcome.files.push(path);
            }
            Err(err) => {
                emit(tx, notify, Level::Error, err.clone());
                outcome.failures.push(format!("{} — {err}", track.title));
            }
        }
        let _ = tx.send(Event::Progress((index + 1) as f32 / total as f32));
        notify();
    }

    let _ = std::fs::remove_dir_all(&temp);

    if outcome.cancelled {
        emit(
            tx,
            notify,
            Level::Warn,
            "Interrompido antes da próxima faixa.",
        );
    }

    if !outcome.failures.is_empty() {
        let report = request.out_dir.join("faixas-nao-baixadas.txt");
        let body = format!(
            "Faixas sem fonte de áudio ({}/{}):\n\n{}",
            outcome.failures.len(),
            total,
            outcome.failures.join("\n")
        );
        let _ = std::fs::write(&report, body);
        emit(
            tx,
            notify,
            Level::Warn,
            format!(
                "{} faixa(s) sem fonte. Lista em {}",
                outcome.failures.len(),
                report.display()
            ),
        );
    }

    if outcome.files.is_empty() && !outcome.cancelled {
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
    let candidates = ytdlp::find_candidates(ytdlp, &track.title, track.duration);
    flush_cookie_problem(ytdlp, tx, notify);
    let candidates = candidates?;
    let mut last_error = String::from("nenhum candidato");

    for (index, candidate) in candidates.iter().enumerate() {
        let template = temp.join(format!("track-{index}.%(ext)s"));
        let attempt = ytdlp.download_audio(&ytdlp::media_url(candidate), &template);
        flush_cookie_problem(ytdlp, tx, notify);
        match attempt {
            Ok(()) => {
                let produced = newest_file(temp, &["mp3"])
                    .ok_or_else(|| "o download terminou sem gerar um MP3".to_string())?;
                match fetch_cover(&track.cover, temp) {
                    Ok(cover) => {
                        if let Err(err) = tags::tag_mp3(
                            &produced,
                            &track.name,
                            &track.artist,
                            &track.album,
                            cover.as_deref(),
                        ) {
                            emit(tx, notify, Level::Warn, format!("metadados: {err}"));
                        }
                    }
                    Err(err) => emit(tx, notify, Level::Warn, err),
                }
                let destination = out_dir.join(format!("{}.mp3", sanitize(&track.title)));
                move_file(&produced, &destination)?;
                return Ok(destination);
            }
            Err(err) => {
                emit(
                    tx,
                    notify,
                    Level::Warn,
                    format!("{}: {err}", short_source(&ytdlp::media_url(candidate))),
                );
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

/// Explain, once, why the browser cookies were not used.
///
/// yt-dlp already retried without them; the point of the message is that
/// Chrome and Edge on Windows cannot be read at all (App-Bound Encryption),
/// which is not something a retry or an update of yt-dlp will fix.
fn flush_cookie_problem(ytdlp: &YtDlp, tx: &Sender<Event>, notify: &impl Fn()) {
    let Some(problem) = ytdlp.take_cookie_problem() else {
        return;
    };
    let lower = problem.to_lowercase();
    let reason = if lower.contains("dpapi") || lower.contains("decrypt") {
        "Chrome/Edge no Windows cifram os cookies com App-Bound Encryption e o yt-dlp não \
         consegue decifrá-los (yt-dlp#10927)."
    } else if lower.contains("could not copy") || lower.contains("unable to open cookie") {
        "o navegador está aberto e travou o banco de cookies."
    } else if lower.contains("could not find") || lower.contains("cookies database") {
        "não achei o banco de cookies desse navegador."
    } else {
        "o yt-dlp não conseguiu ler os cookies."
    };
    emit(
        tx,
        notify,
        Level::Warn,
        format!(
            "Sem cookies: {reason} Seguindo sem sessão; se o YouTube pedir login, use Firefox \
             (instalado e logado) ou um cookies.txt."
        ),
    );
}

fn download_direct_audio(
    ytdlp: &YtDlp,
    request: &Request,
    tx: &Sender<Event>,
    notify: &impl Fn(),
) -> Result<Outcome, String> {
    log(tx, notify, "Baixando o áudio...");
    let temp = temporary_dir();
    std::fs::create_dir_all(&temp).map_err(|err| format!("{err}"))?;

    let template = temp.join("audio.%(ext)s");
    let attempt = ytdlp.download_audio(&request.url, &template);
    flush_cookie_problem(ytdlp, tx, notify);
    attempt?;
    let produced = newest_file(&temp, &["mp3"])
        .ok_or_else(|| "o download terminou sem gerar um MP3".to_string())?;
    let destination = request.out_dir.join(format!("{}.mp3", sanitize("audio")));
    move_file(&produced, &destination)?;
    let _ = std::fs::remove_dir_all(&temp);
    let _ = tx.send(Event::Progress(1.0));
    notify();
    Ok(Outcome {
        files: vec![destination],
        failures: Vec::new(),
        cancelled: false,
    })
}

fn download_direct_video(
    ytdlp: &YtDlp,
    request: &Request,
    tx: &Sender<Event>,
    notify: &impl Fn(),
) -> Result<Outcome, String> {
    log(tx, notify, "Baixando o vídeo...");
    let temp = temporary_dir();
    std::fs::create_dir_all(&temp).map_err(|err| format!("{err}"))?;

    let template = temp.join("video.%(ext)s");
    let attempt = ytdlp.download_video(&request.url, &template);
    flush_cookie_problem(ytdlp, tx, notify);
    attempt?;
    let produced = newest_file(&temp, &["mp4", "mkv", "webm"])
        .ok_or_else(|| "o download terminou sem gerar um vídeo".to_string())?;
    let extension = produced
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4")
        .to_string();
    let destination = request
        .out_dir
        .join(format!("{}.{extension}", sanitize("video")));
    move_file(&produced, &destination)?;
    let _ = std::fs::remove_dir_all(&temp);
    let _ = tx.send(Event::Progress(1.0));
    notify();
    Ok(Outcome {
        files: vec![destination],
        failures: Vec::new(),
        cancelled: false,
    })
}
