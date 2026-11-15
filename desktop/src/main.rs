#![cfg_attr(windows, windows_subsystem = "windows")]
mod client;
mod install;
use eframe::egui;
use std::sync::mpsc;

struct App {
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
        self.failed = false;
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
                        self.status =
                            "Have fun! Infuser Play is keeping your game connected.".into();
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
        egui::CentralPanel::default().show(ctx,|ui|{
            ui.add_space(14.0);
            ui.heading(egui::RichText::new("Infuser Play").size(32.0).color(egui::Color32::from_rgb(159,224,201)));
            ui.add_space(18.0);
            ui.label(egui::RichText::new(&self.status).size(17.0));
            ui.add_space(16.0);
            if self.running {ui.spinner();}
            else if let Some(request)=self.request.clone(){
                if !self.failed {
                    ui.label(format!("Connect to {}",request.server));
                    ui.small("Only connect to an Infuser server you know. This allows its games to run on your computer.");
                    if ui.button("Connect & sign in").clicked(){self.launch(ctx);}
                }else{
                    if ui.button("Back to Library").clicked(){let _=open::that(format!("{}/library",request.server));}
                    if ui.button("Switch player / reconnect").clicked(){
                        match client::forget(&self.root,&request){
                            Ok(())=>{self.failed=false;self.status="Connect with your Google account, then press Play in the Library again.".into();},
                            Err(error)=>self.status=error.to_string(),
                        }
                    }
                }
            }
        });
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
        Ok(request) => (request, "Your games, ready to go.".to_owned(), false),
        Err(error) => (None, error.to_string(), true),
    };
    if argument.is_none() {
        status = "Infuser Play is ready. Open Infuser in your browser and press Play.".into();
    }
    if let Some(error) = storage_error {
        request = None;
        status = error;
        failed = true;
    }
    let auto = request
        .as_ref()
        .is_some_and(|r| client::connected(&root, r));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Infuser Play")
            .with_inner_size([440.0, 280.0])
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        "Infuser Play",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            let mut app = App {
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
