//! ClipHarbor Desktop — a small native downloader for Windows.
//!
//! Everything that touches YouTube/Spotify metadata lives in the other modules;
//! this file is the window, plus a `--cli` mode used for testing.

mod deps;
mod job;
mod spotify;
mod tags;
mod ytdlp;

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use job::{Event, Format};

fn default_folder() -> String {
    let base = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("Downloads")
        .join("ClipHarbor")
        .display()
        .to_string()
}

fn open_folder(path: &str) {
    #[cfg(windows)]
    let _ = deps::command(std::path::Path::new("explorer"))
        .arg(path)
        .spawn();
    #[cfg(not(windows))]
    let _ = deps::command(std::path::Path::new("xdg-open"))
        .arg(path)
        .spawn();
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(position) = args.iter().position(|arg| arg == "--cli") {
        let url = args.get(position + 1).cloned().unwrap_or_default();
        let folder = args
            .get(position + 2)
            .cloned()
            .unwrap_or_else(default_folder);
        return run_cli(&url, &folder);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([780.0, 620.0])
            .with_min_inner_size([560.0, 420.0]),
        ..Default::default()
    };

    eframe::run_native(
        "ClipHarbor",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

fn run_cli(url: &str, folder: &str) -> eframe::Result<()> {
    if url.trim().is_empty() {
        eprintln!("uso: clipharbor-desktop --cli <url> [pasta]");
        std::process::exit(2);
    }

    let (tx, rx) = channel();
    let cookies = std::env::var("CLIPHARBOR_COOKIES").ok();
    {
        let url = url.to_string();
        let folder = PathBuf::from(folder);
        let tx = tx.clone();
        std::thread::spawn(move || {
            job::run(url, Format::Audio, cookies, folder, tx, || {});
        });
    }

    let mut failed = false;
    let mut files = 0;
    while let Ok(event) = rx.recv() {
        match event {
            Event::Log(line) => println!("{line}"),
            Event::Progress(value) => println!("progresso: {:.0}%", value * 100.0),
            Event::Finished(outcome) => {
                files = outcome.files.len();
                for file in &outcome.files {
                    println!("arquivo: {}", file.display());
                }
                println!(
                    "resumo: {} baixada(s), {} falha(s)",
                    outcome.files.len(),
                    outcome.failures.len()
                );
                break;
            }
            Event::Failed(err) => {
                eprintln!("erro: {err}");
                failed = true;
                break;
            }
        }
    }

    std::process::exit(if failed || files == 0 { 1 } else { 0 });
}

struct App {
    url: String,
    folder: String,
    cookies: String,
    /// Browsers with a profile here; offering a missing one breaks every track.
    browsers: Vec<&'static str>,
    format: Format,
    log_lines: Vec<String>,
    receiver: Option<Receiver<Event>>,
    sender: Option<Sender<Event>>,
    running: bool,
    progress: f32,
    summary: String,
}

impl App {
    fn new() -> Self {
        let browsers = deps::installed_browsers();
        let cookies = browsers
            .first()
            .map(|name| name.to_string())
            .unwrap_or_else(|| "nenhum".to_string());
        Self {
            url: String::new(),
            folder: default_folder(),
            cookies,
            browsers,
            format: Format::Audio,
            log_lines: Vec::new(),
            receiver: None,
            sender: None,
            running: false,
            progress: 0.0,
            summary: String::new(),
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let url = self.url.trim().to_string();
        if url.is_empty() {
            self.push_log("Cole um link do Spotify, YouTube ou outro site.");
            return;
        }

        let (tx, rx) = channel();
        self.receiver = Some(rx);
        self.sender = Some(tx.clone());
        self.running = true;
        self.progress = 0.0;
        self.summary.clear();

        let cookies = Some(self.cookies.clone()).filter(|value| value != "nenhum");
        let folder = PathBuf::from(self.folder.trim());
        let format = self.format;
        let notify_ctx = ctx.clone();
        std::thread::spawn(move || {
            job::run(url, format, cookies, folder, tx, move || {
                notify_ctx.request_repaint();
            });
        });
    }

    fn push_log(&mut self, line: impl Into<String>) {
        self.log_lines.push(line.into());
        if self.log_lines.len() > 400 {
            self.log_lines.remove(0);
        }
    }

    fn poll(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.receiver {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }

        for event in events {
            match event {
                Event::Log(line) => self.push_log(line),
                Event::Progress(value) => self.progress = value,
                Event::Finished(outcome) => {
                    self.running = false;
                    self.summary = format!(
                        "{} arquivo(s) salvo(s), {} falha(s).",
                        outcome.files.len(),
                        outcome.failures.len()
                    );
                }
                Event::Failed(err) => {
                    self.running = false;
                    self.summary = format!("Falhou: {err}");
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("ClipHarbor");
            ui.label("Baixa áudio do Spotify, vídeos e outros links neste computador.");
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label("Link:");
                ui.add_sized(
                    [ui.available_width() - 10.0, 24.0],
                    egui::TextEdit::singleline(&mut self.url)
                        .hint_text("https://open.spotify.com/track/... ou https://youtu.be/..."),
                );
            });

            ui.horizontal(|ui| {
                ui.label("Pasta:");
                ui.add_sized(
                    [(ui.available_width() - 190.0).max(160.0), 24.0],
                    egui::TextEdit::singleline(&mut self.folder),
                );
                if ui.button("Abrir pasta").clicked() {
                    open_folder(&self.folder);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Cookies do navegador:");
                egui::ComboBox::from_id_salt("cookies")
                    .selected_text(&self.cookies)
                    .show_ui(ui, |ui| {
                        for option in &self.browsers {
                            ui.selectable_value(&mut self.cookies, option.to_string(), *option);
                        }
                        ui.selectable_value(&mut self.cookies, "nenhum".to_string(), "nenhum");
                    });
                if self.browsers.is_empty() {
                    ui.label("(nenhum navegador com perfil encontrado; baixando sem cookies)");
                } else {
                    ui.label("(escolha o navegador logado no YouTube)");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Formato:");
                ui.selectable_value(&mut self.format, Format::Audio, "🎵 MP3");
                ui.selectable_value(&mut self.format, Format::Video, "🎬 MP4");
                ui.separator();
                let button = egui::Button::new(if self.running {
                    "Baixando..."
                } else {
                    "Baixar"
                });
                if ui.add_enabled(!self.running, button).clicked() {
                    self.start(ctx);
                    self.progress = 0.0;
                }
            });

            if self.running || self.progress > 0.0 {
                ui.add(egui::ProgressBar::new(self.progress).show_percentage());
            }

            if !self.summary.is_empty() {
                ui.add_space(4.0);
                ui.label(&self.summary);
            }

            ui.add_space(6.0);
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &self.log_lines {
                        ui.label(egui::RichText::new(line).monospace().size(12.0));
                    }
                });
        });
    }
}
