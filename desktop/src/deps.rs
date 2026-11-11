//! Locating and bootstrapping the external tools the downloader shells out to.
//!
//! The app itself stays small: `yt-dlp` and `ffmpeg` are downloaded once into
//! `%LOCALAPPDATA%\ClipHarbor\bin` (or used from PATH / next to the exe), which
//! also keeps the YouTube extractor easy to update without rebuilding.

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn bin_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CLIPHARBOR_BIN_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("ClipHarbor").join("bin")
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn file_name(tool: &str) -> String {
    if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_string()
    }
}

/// Look for a tool in the env override, the app folders and then PATH.
pub fn find_tool(tool: &str, env_var: &str) -> Option<PathBuf> {
    if let Ok(path) = std::env::var(env_var) {
        if Path::new(&path).is_file() {
            return Some(PathBuf::from(path));
        }
    }

    let file = file_name(tool);
    for dir in [bin_dir(), exe_dir()] {
        let candidate = dir.join(&file);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(&file);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Do not flash a console window for every yt-dlp call.
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn stderr_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("sem detalhes")
        .trim()
        .to_string()
}

pub fn curl_text(url: &str) -> Result<String, String> {
    let out = command(Path::new("curl"))
        .args([
            "-sSL",
            "--max-time",
            "30",
            "-A",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
            url,
        ])
        .output()
        .map_err(|err| format!("curl não pôde ser executado: {err}"))?;

    if !out.status.success() {
        return Err(format!("Falha ao buscar {url}: {}", stderr_tail(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn curl_download(url: &str, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("{err}"))?;
    }
    let status = command(Path::new("curl"))
        .args([
            "-sSL",
            "--fail",
            "--retry",
            "2",
            "--max-time",
            "900",
            "-o",
            &destination.display().to_string(),
            url,
        ])
        .status()
        .map_err(|err| format!("curl não pôde ser executado: {err}"))?;

    if !status.success() {
        let _ = std::fs::remove_file(destination);
        return Err(format!("Falha ao baixar {url}"));
    }
    Ok(())
}

fn extract_zip(archive: &Path, destination: &Path) -> Result<(), String> {
    let archive_s = archive.display().to_string();
    let destination_s = destination.display().to_string();

    let attempts: Vec<Command> = {
        let mut list = vec![];
        let mut tar = command(Path::new("tar"));
        tar.args(["-xf", &archive_s, "-C", &destination_s]);
        list.push(tar);

        if cfg!(windows) {
            let mut powershell = command(Path::new("powershell"));
            powershell.args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Expand-Archive -LiteralPath '{archive_s}' -DestinationPath '{destination_s}' -Force"
                ),
            ]);
            list.push(powershell);
        } else {
            let mut unzip = command(Path::new("unzip"));
            unzip.args(["-q", "-o", &archive_s, "-d", &destination_s]);
            list.push(unzip);
        }
        list
    };

    for mut attempt in attempts {
        if let Ok(status) = attempt.status() {
            if status.success() {
                return Ok(());
            }
        }
    }
    Err("Não consegui extrair o arquivo baixado.".into())
}

fn find_below(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(path);
        }
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.into_iter().find_map(|dir| find_below(&dir, name, depth - 1))
}

pub fn ensure_ytdlp(mut log: impl FnMut(&str)) -> Result<PathBuf, String> {
    if let Some(path) = find_tool("yt-dlp", "CLIPHARBOR_YTDLP") {
        return Ok(path);
    }

    let destination = bin_dir().join(file_name("yt-dlp"));
    log("Baixando o yt-dlp (só na primeira vez)...");
    curl_download(
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe",
        &destination,
    )?;
    log("yt-dlp pronto.");
    Ok(destination)
}

pub fn ensure_ffmpeg(mut log: impl FnMut(&str)) -> Result<Option<PathBuf>, String> {
    if let Some(path) = find_tool("ffmpeg", "CLIPHARBOR_FFMPEG") {
        return Ok(Some(path));
    }
    if !cfg!(windows) {
        return Ok(None);
    }

    let dir = bin_dir();
    let archive = dir.join("ffmpeg.zip");
    log("Baixando o ffmpeg (só na primeira vez, ~100 MB)...");
    curl_download(
        "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
        &archive,
    )?;

    let extract_dir = dir.join("ffmpeg-extract");
    std::fs::create_dir_all(&extract_dir).map_err(|err| format!("{err}"))?;
    extract_zip(&archive, &extract_dir)?;
    let _ = std::fs::remove_file(&archive);

    let found = find_below(&extract_dir, "ffmpeg.exe", 4)
        .ok_or_else(|| "O ffmpeg não veio no arquivo baixado.".to_string())?;
    let destination = dir.join("ffmpeg.exe");
    std::fs::rename(&found, &destination).map_err(|err| format!("{err}"))?;
    let _ = std::fs::remove_dir_all(&extract_dir);
    log("ffmpeg pronto.");
    Ok(Some(destination))
}
