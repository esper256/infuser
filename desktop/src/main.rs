#![cfg_attr(windows, windows_subsystem = "windows")]
mod client;
mod install;
mod ui;
use eframe::egui;
use std::sync::mpsc;

struct App {
    presentation: ui::Presentation,
    root: std::path::PathBuf,
    request: Option<client::Request>,
    status: String,
    running: bool,
    playing: bool,
    failed: bool,
    receiver: Option<mpsc::Receiver<client::Event>>,
}
impl App {
    fn launch(&mut self, ctx: &egui::Context) {
        let Some(request) = self.request.clone() else {
            return;
        };
        self.running = true;
        self.playing = false;
        self.failed = false;
        self.status = "Connecting…".into();
        let root = self.root.clone();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            if let Err(error) = client::run(root, request, sender.clone()) {
                sender.send(client::Event::Error(error.to_string())).ok();
            }
            repaint.request_repaint();
        });
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(receiver) = &self.receiver {
            while let Ok(event) = receiver.try_recv() {
                match event {
                    client::Event::Status(message) => self.status = message,
                    client::Event::Playing => {
                        self.playing = true;
                        self.status = "Game running".into();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                    client::Event::Done => {
                        self.running = false;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    client::Event::Error(message) => {
                        self.running = false;
                        self.failed = true;
                        self.status = message;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    }
                }
            }
        }
        if self.playing && self.running && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.playing {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }
        let screen = if self.failed {
            ui::Screen::Error {
                message: &self.status,
                has_request: self.request.is_some(),
            }
        } else if self.playing && self.running {
            ui::Screen::Playing
        } else if self.running {
            ui::Screen::Busy(&self.status)
        } else if let Some(request) = &self.request {
            ui::Screen::Connect {
                server: &request.server,
                note: &self.status,
            }
        } else {
            ui::Screen::Idle
        };
        match self.presentation.show(ctx, screen) {
            ui::Action::None => {}
            ui::Action::Connect => self.launch(ctx),
            ui::Action::Library => {
                if let Some(request) = &self.request {
                    let _ = open::that(format!("{}/library", request.server));
                }
            }
            ui::Action::Reconnect => {
                if let Some(request) = &self.request {
                    match client::forget(&self.root, request) {
                        Ok(()) => {
                            self.failed = false;
                            self.status = "Sign in, then press Play in the Library again.".into();
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
        }
        if self.running {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}
fn main() -> eframe::Result {
    let storage = client::root();
    let storage_error = storage.as_ref().err().map(ToString::to_string);
    let root = storage.unwrap_or_default();
    let argument = std::env::args().nth(1);
    let parsed = argument.as_deref().map(client::parse).transpose();
    let (mut request, mut status, mut failed) = match parsed {
        Ok(request) => (request, String::new(), false),
        Err(error) => (None, error.to_string(), true),
    };
    if let Some(error) = storage_error {
        request = None;
        status = error;
        failed = true;
    }
    let auto = request
        .as_ref()
        .is_some_and(|r| client::connected(&root, r));
    let options = eframe::NativeOptions {
        viewport: ui::viewport(),
        ..Default::default()
    };
    eframe::run_native(
        "Infuser Play",
        options,
        Box::new(move |cc| {
            let mut app = App {
                presentation: ui::Presentation::new(&cc.egui_ctx),
                root,
                request,
                status,
                running: false,
                playing: false,
                failed,
                receiver: None,
            };
            if auto {
                app.launch(&cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )
}
