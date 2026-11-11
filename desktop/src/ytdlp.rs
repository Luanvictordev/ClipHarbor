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
    /// Set when the cookies could not be read and the app went on without them.
    cookie_problem: Cell<Option<String>>,
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
    pub fn new(exe: PathBuf, ffmpeg: Option<PathBuf>, cookies: Option<String>) -> Self {
        Self {
            exe,
            ffmpeg,
            cookies: cookies.filter(|value| !value.trim().is_empty()),
            cookie_problem: Cell::new(None),
        }
    }

    /// The cookies problem noticed during the last calls, reported once.
    pub fn take_cookie_problem(&self) -> Option<String> {
        self.cookie_problem.take()
    }

    /// Run `attempt`, retrying once without cookies if yt-dlp cannot read them.
    fn with_cookie_fallback<T>(
        &self,
        attempt: impl Fn(Option<&str>) -> Result<T, String>,
    ) -> Result<T, String> {
        let chosen = self.cookies.as_deref();
        match attempt(chosen) {
            Err(err) if chosen.is_some() && is_cookie_error(&err) => {
                self.cookie_problem.set(Some(err));
                attempt(None)
            }
            other => other,
        }
    }

    fn command(&self, cookies: Option<&str>) -> std::process::Command {
        let mut cmd = deps::command(&self.exe);
        cmd.arg("--no-playlist")
            .arg("--no-color")
            .arg("--no-warnings");
        if let Some(browser) = cookies.filter(|value| !value.is_empty()) {
            cmd.arg("--cookies-from-browser").arg(browser);
        }
        cmd
    }

    pub fn search(&self, query: &str) -> Result<Vec<Entry>, String> {
        self.with_cookie_fallback(|cookies| self.search_with(query, cookies))
    }

    fn search_with(&self, query: &str, cookies: Option<&str>) -> Result<Vec<Entry>, String> {
        let out = self
            .command(cookies)
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
        self.with_cookie_fallback(|cookies| self.download_audio_with(media_url, output, cookies))
    }

    fn download_audio_with(
        &self,
        media_url: &str,
        output: &Path,
        cookies: Option<&str>,
    ) -> Result<(), String> {
        let mut cmd = self.command(cookies);
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
        self.with_cookie_fallback(|cookies| self.download_video_with(media_url, output, cookies))
    }

    fn download_video_with(
        &self,
        media_url: &str,
        output: &Path,
        cookies: Option<&str>,
    ) -> Result<(), String> {
        let mut cmd = self.command(cookies);
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
