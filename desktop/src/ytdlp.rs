//! Thin wrapper around yt-dlp: search, candidate ranking and the download call.
//!
//! Rust has no maintained YouTube extractor, and this one changes constantly,
//! so the app drives the `yt-dlp` binary and only keeps the part that is its own:
//! matching a Spotify track to the right audio.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::deps;

#[derive(Clone, Debug, Deserialize, Default)]
pub struct Entry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub webpage_url: Option<String>,
}

#[derive(Deserialize)]
struct SearchPayload {
    #[serde(default)]
    entries: Option<Vec<Entry>>,
}

pub struct YtDlp {
    exe: PathBuf,
    ffmpeg: Option<PathBuf>,
    /// Browser whose cookies yt-dlp should read, e.g. "firefox".
    cookies: Option<String>,
    /// Netscape `cookies.txt`, for browsers yt-dlp cannot read directly.
    cookies_file: Option<PathBuf>,
    /// Set when the cookies could not be read and the app went on without them.
    cookie_problem: Cell<Option<String>>,
    /// Once set, cookies are not passed at all for the rest of the run.
    cookies_broken: Cell<bool>,
}

/// Errors that mean "these cookies are unusable", not "this track failed".
///
/// A missing browser, a locked database or undecryptable cookies hit every
/// track the same way, so they must switch the download to no cookies instead
/// of failing the whole album.
pub fn is_cookie_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("cookies database")
        || lower.contains("cookie database")
        || (lower.contains("could not find") && lower.contains("cookie"))
        || lower.contains("cookies-from-browser")
        || lower.contains("failed to decrypt")
        || lower.contains("could not copy")
        || lower.contains("unable to open cookie")
}

const NOISE_WORDS: &[&str] = &[
    "sped up",
    "speed up",
    "slowed",
    "nightcore",
    "reverb",
    "remix",
    "cover",
    "instrumental",
    "karaoke",
    "8d audio",
    "bass boost",
    "mashup",
    "live",
    "tribute",
    "made famous by",
    "in the style of",
    "re-recorded",
];

impl YtDlp {
    pub fn new(
        exe: PathBuf,
        ffmpeg: Option<PathBuf>,
        cookies: Option<String>,
        cookies_file: Option<PathBuf>,
    ) -> Self {
        Self {
            exe,
            ffmpeg,
            cookies: cookies.filter(|value| !value.trim().is_empty()),
            cookies_file,
            cookie_problem: Cell::new(None),
            cookies_broken: Cell::new(false),
        }
    }

    /// The cookies problem noticed during the last calls, reported once.
    pub fn take_cookie_problem(&self) -> Option<String> {
        self.cookie_problem.take()
    }

    fn has_cookies(&self) -> bool {
        self.cookies.is_some() || self.cookies_file.is_some()
    }

    /// Run `attempt`, retrying without cookies when yt-dlp cannot read them.
    ///
    /// The retry is remembered for the rest of the run: a broken cookie store
    /// would otherwise make every search and download pay for the same failing
    /// attempt and repeat the same warning.
    fn with_cookie_fallback<T>(
        &self,
        attempt: impl Fn() -> Result<T, String>,
    ) -> Result<T, String> {
        match attempt() {
            Err(err)
                if self.has_cookies() && !self.cookies_broken.get() && is_cookie_error(&err) =>
            {
                self.cookies_broken.set(true);
                self.cookie_problem.set(Some(err));
                attempt()
            }
            other => other,
        }
    }

    fn command(&self) -> std::process::Command {
        let mut cmd = deps::command(&self.exe);
        cmd.arg("--no-playlist")
            .arg("--no-color")
            .arg("--no-warnings");
        if self.cookies_broken.get() {
            return cmd;
        }
        if let Some(browser) = self.cookies.as_deref() {
            cmd.arg("--cookies-from-browser").arg(browser);
        } else if let Some(file) = &self.cookies_file {
            cmd.arg("--cookies").arg(file);
        }
        cmd
    }

    pub fn search(&self, query: &str) -> Result<Vec<Entry>, String> {
        self.with_cookie_fallback(|| self.search_once(query))
    }

    fn search_once(&self, query: &str) -> Result<Vec<Entry>, String> {
        let out = self
            .command()
            .arg("--flat-playlist")
            .arg("-J")
            .arg(query)
            .output()
            .map_err(|err| format!("não consegui executar o yt-dlp: {err}"))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let last = stderr
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("busca falhou");
            return Err(last.trim().to_string());
        }

        let payload: SearchPayload = serde_json::from_slice(&out.stdout)
            .map_err(|err| format!("resposta do yt-dlp não entendida: {err}"))?;
        Ok(payload.entries.unwrap_or_default())
    }

    pub fn download_audio(&self, media_url: &str, output: &Path) -> Result<(), String> {
        self.with_cookie_fallback(|| self.download_audio_once(media_url, output))
    }

    fn download_audio_once(&self, media_url: &str, output: &Path) -> Result<(), String> {
        let mut cmd = self.command();
        cmd.args([
            "-x",
            "--audio-format",
            "mp3",
            "--audio-quality",
            "0",
            "--no-progress",
        ])
        .arg("-o")
        .arg(output);
        if let Some(ffmpeg) = &self.ffmpeg {
            cmd.arg("--ffmpeg-location").arg(ffmpeg);
        }
        finish(cmd.arg(media_url))
    }

    pub fn download_video(&self, media_url: &str, output: &Path) -> Result<(), String> {
        self.with_cookie_fallback(|| self.download_video_once(media_url, output))
    }

    fn download_video_once(&self, media_url: &str, output: &Path) -> Result<(), String> {
        let mut cmd = self.command();
        cmd.args([
            "-f",
            "bestvideo+bestaudio/best",
            "--merge-output-format",
            "mp4",
            "--no-progress",
        ])
        .arg("-o")
        .arg(output);
        if let Some(ffmpeg) = &self.ffmpeg {
            cmd.arg("--ffmpeg-location").arg(ffmpeg);
        }
        finish(cmd.arg(media_url))
    }
}

fn finish(cmd: &mut std::process::Command) -> Result<(), String> {
    let out = cmd
        .output()
        .map_err(|err| format!("não consegui executar o yt-dlp: {err}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let errors: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| line.contains("ERROR:"))
        .collect();
    let message = errors
        .last()
        .map(|line| line.to_string())
        .unwrap_or_else(|| {
            stderr
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("o download falhou")
                .trim()
                .to_string()
        });
    Err(message)
}

fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| word.len() > 2)
        .map(str::to_string)
        .collect()
}

/// Order search hits by how well they match the expected track.
///
/// Duration is the main signal: a 30 second preview or a different edit must
/// never win over the real track.
pub fn rank(entries: &[Entry], expected_duration: Option<u64>, query: &str) -> Vec<Entry> {
    let query_words = tokens(query);
    let lowered_query = query.to_lowercase();
    let mut scored: Vec<(f64, Entry)> = Vec::new();

    for (position, entry) in entries.iter().enumerate() {
        let title = entry.title.to_lowercase();
        let mut score = position as f64 * 0.5;

        if let Some(duration) = entry.duration {
            if duration > 0.0 {
                if let Some(expected) = expected_duration {
                    let expected = expected as f64;
                    let difference = (duration - expected).abs();
                    if difference > (20.0f64).max(expected * 0.15) {
                        continue;
                    }
                    score += difference;
                }
                if duration < 45.0 && expected_duration.unwrap_or(0) > 60 {
                    continue;
                }
            }
        }

        if NOISE_WORDS
            .iter()
            .any(|noise| title.contains(noise) && !lowered_query.contains(noise))
        {
            continue;
        }

        let title_words = tokens(&title);
        let shared = title_words
            .iter()
            .filter(|word| query_words.contains(word))
            .count();
        score -= 3.0 * shared as f64;
        let extra = title_words
            .iter()
            .filter(|word| !query_words.contains(word))
            .count();
        score += 2.5 * extra as f64;

        scored.push((score, entry.clone()));
    }

    scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|(_, entry)| entry).collect()
}

/// Ranked candidates for a Spotify track: YouTube first (best catalog on a home
/// connection), SoundCloud as the fallback that still works without cookies.
pub fn find_candidates(
    ytdlp: &YtDlp,
    query: &str,
    duration: Option<u64>,
) -> Result<Vec<Entry>, String> {
    let mut candidates = Vec::new();
    let mut problems = Vec::new();

    for prefix in ["ytsearch5:", "scsearch5:"] {
        match ytdlp.search(&format!("{prefix}{query}")) {
            Ok(entries) => {
                let ranked = rank(&entries, duration, query);
                if ranked.is_empty() {
                    problems.push(format!("{prefix} sem resultado compatível"));
                }
                candidates.extend(ranked.into_iter().take(2));
            }
            Err(err) => problems.push(format!("{prefix} {err}")),
        }
    }

    if candidates.is_empty() {
        return Err(format!(
            "Nenhuma fonte de áudio funcionou para esta faixa ({})",
            problems.join(" | ")
        ));
    }
    Ok(candidates)
}

pub fn media_url(entry: &Entry) -> String {
    entry
        .webpage_url
        .clone()
        .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={}", entry.id))
}
