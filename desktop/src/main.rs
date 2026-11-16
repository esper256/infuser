#![cfg_attr(windows, windows_subsystem = "windows")]
mod client;
mod install;
mod run_storage;
mod ui;
mod update;
use eframe::egui;
use std::{
    process::Child,
    sync::{Arc, Mutex, mpsc},
};

struct App {
    startup: Option<mpsc::Receiver<Option<Child>>>,
    handoff: Arc<Mutex<Option<Child>>>,
    auto_launch: bool,
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
        if let Some(receiver) = &self.startup {
            match receiver.try_recv() {
                Ok(handed_off) => {
                    self.startup = None;
                    self.running = false;
                    if let Some(child) = handed_off {
                        *self.handoff.lock().unwrap() = Some(child);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        return;
                    }
                    self.status.clear();
                    if self.auto_launch {
                        self.launch(ctx);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.startup = None;
                    self.running = false;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
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
    let mut arguments: Vec<_> = std::env::args().skip(1).collect();
    let managed = arguments
        .first()
        .is_some_and(|arg| arg == "--infuser-managed");
    if managed {
        arguments.remove(0);
    }
    let argument = arguments.first().cloned();
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
    let handoff = Arc::new(Mutex::new(None::<Child>));
    let app_handoff = handoff.clone();
    let result = eframe::run_native(
        "Infuser Play",
        options,
        Box::new(move |cc| {
            let startup = if !managed && !failed {
                let (sender, receiver) = mpsc::channel();
                let storage = root.clone();
                let repaint = cc.egui_ctx.clone();
                std::thread::spawn(move || {
                    let _ = sender.send(update::run(&storage, &arguments));
                    repaint.request_repaint();
                });
                status = "Checking Infuser Play updates…".into();
                Some(receiver)
            } else {
                None
            };
            let updating = startup.is_some();
            let mut app = App {
                startup,
                handoff: app_handoff,
                auto_launch: auto,
                presentation: ui::Presentation::new(&cc.egui_ctx),
                root,
                request,
                status,
                running: updating,
                playing: false,
                failed,
                receiver: None,
            };
            if managed {
                update::acknowledge();
            }
            if auto && !updating {
                app.launch(&cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    );
    // Closing the launcher's window must not end Flatpak's process namespace.
    // Drop all GUI resources, then quietly keep the sandbox alive for the worker.
    if let Some(mut child) = handoff.lock().unwrap().take() {
        let _ = child.wait();
    }
    result
}
