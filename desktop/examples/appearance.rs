//! Offline visual review of the real native UI. No account, storage or network access.
//! cargo run --manifest-path desktop/Cargo.toml --example appearance -- connect
//! Optional second argument saves a screenshot, then exits; third/fourth set width/height.
#[path = "../src/about.rs"]
mod about;
#[path = "../src/ui.rs"]
mod ui;
use eframe::egui;

struct Preview {
    presentation: ui::Presentation,
    state: String,
    output: Option<String>,
    frames: usize,
    started: std::time::Instant,
}

impl eframe::App for Preview {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let screen = match self.state.as_str() {
            "connect" => ui::Screen::Connect {
                server: "https://library.example",
                note: "",
            },
            "long-server" => ui::Screen::Connect {
                server: "https://a-long-but-valid-library-hostname.for-the-native-window.example:8443",
                note: "Sign in, then press Play in the Library again.",
            },
            "opening" => ui::Screen::Busy("Downloading Firefly Drift…"),
            "long-title" => ui::Screen::Busy(
                "Downloading The Surprisingly Long Adventure of the Fireflies and Their Friends…",
            ),
            "browser" => ui::Screen::Busy("Finish connecting in your browser…"),
            "playing" => ui::Screen::Playing,
            "error" => ui::Screen::Error {
                message: "The connection was interrupted.",
                has_request: true,
            },
            "long-error" => ui::Screen::Error {
                message: "The game could not be downloaded. The connection to https://a-long-library-hostname.example was interrupted while waiting for a response. Open the Library and press Play again. If this continues, ask the server administrator to check the connection and available storage.",
                has_request: true,
            },
            "invalid-link" => ui::Screen::Error {
                message: "Invalid Infuser launch link.",
                has_request: false,
            },
            "idle" | "about" | "about-failed" => ui::Screen::Idle,
            other => panic!("unknown preview state: {other}"),
        };
        if self.state.starts_with("about") && self.frames == 0 {
            self.presentation.about_open = true;
        }
        let _ = self.presentation.show(ctx, screen, || about::UpdateInfo {
            status: if self.state == "about-failed" {
                "Update check or download failed; using a working version"
            } else {
                "Up to date at last check"
            },
            checked: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            ),
        });
        if let Some(output) = &self.output {
            for event in ctx.input(|input| input.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let bytes: Vec<_> = image
                        .pixels
                        .iter()
                        .flat_map(|pixel| pixel.to_array())
                        .collect();
                    image::save_buffer(
                        output,
                        &bytes,
                        image.width() as u32,
                        image.height() as u32,
                        image::ColorType::Rgba8,
                    )
                    .expect("save screenshot");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            self.frames += 1;
            if self.frames > 3 && self.started.elapsed() > std::time::Duration::from_millis(400) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args().collect();
    let state = args.get(1).cloned().unwrap_or_else(|| "connect".into());
    let output = args.get(2).cloned();
    let width = args
        .get(3)
        .map(|w| w.parse::<f32>().expect("window width"))
        .unwrap_or(480.0);
    let height = args
        .get(4)
        .map(|h| h.parse::<f32>().expect("window height"))
        .unwrap_or(360.0);
    eframe::run_native(
        "Infuser Play preview",
        eframe::NativeOptions {
            viewport: ui::viewport().with_inner_size([width, height]),
            ..Default::default()
        },
        Box::new(move |cc| {
            Ok(Box::new(Preview {
                presentation: ui::Presentation::new(&cc.egui_ctx),
                state,
                output,
                frames: 0,
                started: std::time::Instant::now(),
            }))
        }),
    )
}
