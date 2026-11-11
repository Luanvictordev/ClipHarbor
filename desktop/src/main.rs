//! ClipHarbor Desktop — a small native downloader for Windows.
//!
//! Everything that touches YouTube/Spotify metadata lives in the other modules;
//! this file is the window, plus a `--cli` mode used for testing.

mod deps;
mod job;
mod spotify;
mod tags;
mod theme;
mod ytdlp;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use egui::{Align, Layout, RichText};

use job::{Event, Format, Level, Request};

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

    if args.iter().any(|arg| arg == "--check-layout") {
        return check_layout();
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 780.0])
            .with_min_inner_size([640.0, 560.0])
            .with_title("ClipHarbor"),
        ..Default::default()
    };

    eframe::run_native(
        "ClipHarbor",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

fn run_cli(url: &str, folder: &str) -> eframe::Result<()> {
    if url.trim().is_empty() {
        eprintln!("uso: clipharbor-desktop --cli <url> [pasta]");
        std::process::exit(2);
    }

    let (tx, rx) = channel();
    let request = Request {
        url: url.to_string(),
        format: Format::Audio,
        browser: std::env::var("CLIPHARBOR_COOKIES").ok(),
        cookies_file: std::env::var("CLIPHARBOR_COOKIES_FILE")
            .ok()
            .map(PathBuf::from),
        out_dir: PathBuf::from(folder),
    };
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            job::run(request, cancel, tx, || {});
        });
    }

    let mut failed = false;
    let mut files = 0;
    while let Ok(event) = rx.recv() {
        match event {
            Event::Log(level, line) => println!("{}{line}", prefix(level)),
            Event::Track {
                index,
                total,
                title,
            } => println!("[{index}/{total}] {title}"),
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

fn prefix(level: Level) -> &'static str {
    match level {
        Level::Info => "",
        Level::Ok => "",
        Level::Warn => "aviso: ",
        Level::Error => "erro: ",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Running,
    Stopping,
    Finished,
    Failed,
}

struct App {
    url: String,
    folder: String,
    browsers: Vec<&'static str>,
    browser: String,
    cookies_file: String,
    format: Format,
    log: Vec<(Level, String)>,
    receiver: Option<Receiver<Event>>,
    phase: Phase,
    progress: f32,
    track: Option<(usize, usize, String)>,
    summary: String,
    files: usize,
    failures: usize,
    cancel: Arc<AtomicBool>,
}

impl App {
    fn new() -> Self {
        let browsers = deps::installed_browsers();
        let browser = browsers
            .first()
            .map(|name| name.to_string())
            .unwrap_or_else(|| "nenhum".to_string());
        Self {
            url: String::new(),
            folder: default_folder(),
            browsers,
            browser,
            cookies_file: String::new(),
            format: Format::Audio,
            log: Vec::new(),
            receiver: None,
            phase: Phase::Idle,
            progress: 0.0,
            track: None,
            summary: String::new(),
            files: 0,
            failures: 0,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn push(&mut self, level: Level, line: impl Into<String>) {
        self.log.push((level, line.into()));
        if self.log.len() > 500 {
            self.log.remove(0);
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.phase == Phase::Running || self.phase == Phase::Stopping {
            return;
        }
        let url = self.url.trim().to_string();
        if url.is_empty() {
            self.push(
                Level::Warn,
                "Cole um link do Spotify, YouTube ou outro site.",
            );
            return;
        }

        let cookies_file = self.cookies_file.trim().to_string();
        let request = Request {
            url,
            format: self.format,
            browser: Some(self.browser.clone()),
            cookies_file: if cookies_file.is_empty() {
                None
            } else {
                Some(PathBuf::from(cookies_file))
            },
            out_dir: PathBuf::from(self.folder.trim()),
        };

        let (tx, rx) = channel();
        self.receiver = Some(rx);
        self.phase = Phase::Running;
        self.progress = 0.0;
        self.track = None;
        self.summary.clear();
        self.files = 0;
        self.failures = 0;
        self.cancel = Arc::new(AtomicBool::new(false));

        let cancel = self.cancel.clone();
        let notify_ctx = ctx.clone();
        std::thread::spawn(move || {
            job::run(request, cancel, tx, move || {
                notify_ctx.request_repaint();
            });
        });
    }

    fn stop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.phase = Phase::Stopping;
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
                Event::Log(level, line) => self.push(level, line),
                Event::Track {
                    index,
                    total,
                    title,
                } => self.track = Some((index, total, title)),
                Event::Progress(value) => self.progress = value,
                Event::Finished(outcome) => {
                    self.phase = if outcome.cancelled {
                        Phase::Idle
                    } else if outcome.files.is_empty() {
                        Phase::Failed
                    } else {
                        Phase::Finished
                    };
                    self.files = outcome.files.len();
                    self.failures = outcome.failures.len();
                    self.summary = if outcome.cancelled {
                        format!("Interrompido: {} arquivo(s) salvo(s).", outcome.files.len())
                    } else {
                        format!(
                            "{} arquivo(s) salvo(s), {} falha(s).",
                            outcome.files.len(),
                            outcome.failures.len()
                        )
                    };
                    self.receiver = None;
                }
                Event::Failed(err) => {
                    self.phase = Phase::Failed;
                    self.summary = err.clone();
                    self.push(Level::Error, err);
                    self.receiver = None;
                }
            }
        }
    }

    fn status(&self) -> (&'static str, egui::Color32) {
        match self.phase {
            Phase::Idle => ("Pronto", theme::MUTED),
            Phase::Running => ("Baixando", theme::ACCENT_HOVER),
            Phase::Stopping => ("Parando", theme::WARN),
            Phase::Finished => ("Concluído", theme::OK),
            Phase::Failed => ("Falhou", theme::ERR),
        }
    }

    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("ClipHarbor")
                        .size(21.0)
                        .strong()
                        .color(theme::TEXT),
                );
                theme::muted(ui, "Áudio do Spotify, vídeos e links, neste computador.");
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (text, color) = self.status();
                theme::pill(ui, text, color);
                theme::label(ui, "v0.1", theme::FAINT, 11.5);
            });
        });
    }

    fn form(&mut self, ui: &mut egui::Ui) {
        let mut start = false;

        theme::card(ui, |ui| {
            theme::section(ui, "Link");
            egui::Frame::none()
                .fill(theme::SURFACE_2)
                .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                .rounding(egui::Rounding::same(9.0))
                .inner_margin(egui::Margin::symmetric(4.0, 3.0))
                .show(ui, |ui| {
                    ui.add_sized(
                        [ui.available_width(), 30.0],
                        egui::TextEdit::singleline(&mut self.url)
                            .hint_text(
                                RichText::new(
                                    "https://open.spotify.com/track/…  ou  https://youtu.be/…",
                                )
                                .color(theme::FAINT),
                            )
                            .frame(false)
                            .margin(egui::Margin::symmetric(6.0, 6.0))
                            .font(egui::FontId::new(14.0, egui::FontFamily::Proportional)),
                    );
                });

            ui.add_space(10.0);
            theme::section(ui, "Salvar em");
            ui.horizontal(|ui| {
                let button_width = 104.0;
                ui.add_sized(
                    [(ui.available_width() - button_width - 8.0).max(160.0), 32.0],
                    egui::TextEdit::singleline(&mut self.folder)
                        .margin(egui::Margin::symmetric(10.0, 7.0))
                        .font(egui::FontId::new(14.0, egui::FontFamily::Proportional)),
                );
                if theme::button(ui, "Abrir pasta", theme::Kind::Ghost).clicked() {
                    open_folder(self.folder.trim());
                }
            });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                theme::label(ui, "Formato", theme::FAINT, 10.5);
                let options = [
                    ("MP3", self.format == Format::Audio),
                    ("MP4", self.format == Format::Video),
                ];
                if let Some(choice) = theme::segmented(ui, &options) {
                    self.format = if choice == 0 {
                        Format::Audio
                    } else {
                        Format::Video
                    };
                }

                ui.add_space(12.0);
                theme::label(ui, "Cookies", theme::FAINT, 10.5);
                egui::ComboBox::from_id_salt("cookies")
                    .width(120.0)
                    .selected_text(&self.browser)
                    .show_ui(ui, |ui| {
                        for option in &self.browsers {
                            ui.selectable_value(&mut self.browser, option.to_string(), *option);
                        }
                        ui.selectable_value(&mut self.browser, "nenhum".to_string(), "nenhum");
                    });
            });

            if self.browsers.is_empty() {
                theme::label(
                    ui,
                    "Nenhum navegador com perfil encontrado: os downloads vão sem cookies.",
                    theme::WARN,
                    11.5,
                );
            } else if cfg!(windows)
                && matches!(
                    self.browser.as_str(),
                    "chrome" | "edge" | "brave" | "chromium" | "vivaldi" | "opera"
                )
            {
                theme::label(
                    ui,
                    "Chrome/Edge no Windows protegem os cookies com App-Bound Encryption e o yt-dlp não consegue lê-los. Prefira Firefox ou um cookies.txt.",
                    theme::WARN,
                    11.5,
                );
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                theme::label(ui, "cookies.txt", theme::FAINT, 10.5);
                ui.add_sized(
                    [ui.available_width(), 30.0],
                    egui::TextEdit::singleline(&mut self.cookies_file)
                        .hint_text(
                            RichText::new("opcional: caminho do cookies.txt (formato Netscape)")
                                .color(theme::FAINT),
                        )
                        .margin(egui::Margin::symmetric(10.0, 6.0))
                        .font(egui::FontId::new(12.5, egui::FontFamily::Proportional)),
                );
            });

            ui.add_space(12.0);
            let label = match self.phase {
                Phase::Running => "Parar depois desta faixa",
                Phase::Stopping => "Parando…",
                _ => "Baixar",
            };
            let kind = match self.phase {
                Phase::Running => theme::Kind::Danger,
                Phase::Stopping => theme::Kind::Ghost,
                _ => theme::Kind::Primary,
            };
            let enabled = self.phase != Phase::Stopping;
            let width = ui.available_width();
            ui.add_enabled_ui(enabled, |ui| {
                if theme::button_sized(ui, label, kind, Some(width)).clicked() {
                    match self.phase {
                        Phase::Running => self.stop(),
                        _ => start = true,
                    }
                }
            });
        });

        if start {
            self.start(ui.ctx());
        }
    }

    fn progress_card(&self, ui: &mut egui::Ui) {
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                theme::label(ui, "Progresso", theme::TEXT, 13.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    theme::label(
                        ui,
                        &format!("{:.0}%", self.progress * 100.0),
                        theme::MUTED,
                        12.0,
                    );
                });
            });
            ui.add_space(4.0);
            theme::progress(ui, self.progress);
            ui.add_space(8.0);

            let line = match &self.track {
                Some((index, total, title)) => format!("Faixa {index}/{total} · {title}"),
                None if self.phase == Phase::Running => "Preparando…".to_string(),
                None => "Nada na fila.".to_string(),
            };
            theme::label(ui, &line, theme::MUTED, 12.5);

            if !self.summary.is_empty() {
                ui.add_space(6.0);
                let color = match self.phase {
                    Phase::Failed => theme::ERR,
                    Phase::Finished => theme::OK,
                    _ => theme::MUTED,
                };
                theme::label(ui, &self.summary, color, 12.5);
            }

            if self.files > 0 || self.failures > 0 {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    theme::label(ui, &format!("{} salvos", self.files), theme::OK, 12.0);
                    theme::label(ui, "·", theme::FAINT, 12.0);
                    theme::label(
                        ui,
                        &format!("{} falhas", self.failures),
                        if self.failures > 0 {
                            theme::ERR
                        } else {
                            theme::MUTED
                        },
                        12.0,
                    );
                });
            }
        });
    }

    fn log_card(&mut self, ui: &mut egui::Ui) {
        let mut clear = false;
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                theme::label(ui, "Registro", theme::TEXT, 13.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::button(ui, "Limpar", theme::Kind::Ghost).clicked() {
                        clear = true;
                    }
                });
            });
            ui.add_space(4.0);

            egui::Frame::none()
                .fill(theme::BG)
                .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                .rounding(egui::Rounding::same(9.0))
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(170.0)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if self.log.is_empty() {
                                theme::label(
                                    ui,
                                    "Os detalhes do download aparecem aqui.",
                                    theme::FAINT,
                                    12.0,
                                );
                            }
                            for (level, line) in &self.log {
                                let color = match level {
                                    Level::Info => theme::MUTED,
                                    Level::Ok => theme::OK,
                                    Level::Warn => theme::WARN,
                                    Level::Error => theme::ERR,
                                };
                                ui.label(RichText::new(line).monospace().size(12.0).color(color));
                            }
                        });
                });
        });
        if clear {
            self.log.clear();
        }
    }
}

impl App {
    /// The whole window; returns the content height for `--check-layout`.
    fn draw(&mut self, ctx: &egui::Context) -> f32 {
        self.poll();
        let mut content = 0.0;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(22.0, 20.0)),
            )
            .show(ctx, |ui| {
                // A shorter window scrolls instead of clipping the log away.
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.header(ui);
                        ui.add_space(14.0);
                        self.form(ui);
                        ui.add_space(10.0);
                        self.progress_card(ui);
                        ui.add_space(10.0);
                        self.log_card(ui);
                        content = ui.min_rect().bottom();
                    });
            });
        content
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw(ctx);
    }
}

/// Layout smoke test that needs no display server: the window is rendered to
/// shapes and the geometry is checked against the visible area.
fn check_layout() -> eframe::Result<()> {
    let size = egui::vec2(900.0, 780.0);
    let ctx = egui::Context::default();
    theme::apply(&ctx);

    let mut app = App::new();
    app.url = "https://open.spotify.com/album/1hwPP3qgnV7PQrMSy6qFw8".to_string();
    app.push(Level::Info, "Preparando yt-dlp e ffmpeg...");
    app.push(
        Level::Ok,
        "A Flock Of Seagulls - I Ran (So Far Away).mp3 salvo",
    );
    app.push(
        Level::Warn,
        "Sem cookies: Chrome/Edge no Windows cifram os cookies com App-Bound \
         Encryption e o yt-dlp não consegue decifrá-los (yt-dlp#10927). Seguindo sem \
         sessão; se o YouTube pedir login, use Firefox (instalado e logado) ou um cookies.txt.",
    );
    app.progress = 0.42;
    app.track = Some((4, 10, "A Flock Of Seagulls - Don't Ask Me".to_string()));
    app.files = 3;
    app.phase = Phase::Running;

    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        time: Some(0.0),
        ..Default::default()
    };

    let mut content = 0.0;
    for pass in 1..=2 {
        let output = ctx.run(raw.clone(), |ctx| {
            content = app.draw(ctx);
        });
        println!(
            "passo {pass}: {:.0} x {:.0} de conteúdo (janela {:.0} x {:.0}), {} primitivas",
            size.x,
            content,
            size.x,
            size.y,
            ctx.tessellate(output.shapes, output.pixels_per_point).len()
        );
    }

    let exceeded = content > size.y - 16.0;
    if exceeded {
        eprintln!("layout maior que a janela");
        std::process::exit(1);
    }
    Ok(())
}
