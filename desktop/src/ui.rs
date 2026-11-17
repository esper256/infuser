//! Shared native presentation. Launching, credentials and network state stay in App.
use crate::about::{self, UpdateInfo};
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use std::time::{Duration, Instant};

const BACKGROUND: Color32 = Color32::from_rgb(18, 22, 25);
const SURFACE: Color32 = Color32::from_rgb(32, 38, 42);
const BORDER: Color32 = Color32::from_rgb(52, 62, 66);
const TEXT: Color32 = Color32::from_rgb(241, 240, 231);
const MUTED: Color32 = Color32::from_rgb(169, 182, 184);
const MINT: Color32 = Color32::from_rgb(153, 231, 194);

pub enum Screen<'a> {
    Idle,
    Connect { server: &'a str, note: &'a str },
    Busy(&'a str),
    Playing,
    Error { message: &'a str, has_request: bool },
}

#[derive(Default)]
pub enum Action {
    #[default]
    None,
    Connect,
    Library,
    Reconnect,
}

pub struct Presentation {
    logo: egui::TextureHandle,
    pub about_open: bool,
    about_info: Option<(Instant, UpdateInfo)>,
    copied: bool,
}

pub fn viewport() -> egui::ViewportBuilder {
    let icon = image::load_from_memory(include_bytes!("../../assets/branding/infuser/icon-64.png"))
        .expect("embedded app icon")
        .into_rgba8();
    egui::ViewportBuilder::default()
        .with_title("Infuser Play")
        .with_inner_size([480.0, 360.0])
        .with_min_inner_size([360.0, 360.0])
        .with_icon(egui::IconData {
            width: icon.width(),
            height: icon.height(),
            rgba: icon.into_raw(),
        })
}

impl Presentation {
    pub fn new(ctx: &egui::Context) -> Self {
        ctx.set_theme(egui::Theme::Dark);
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "noto".into(),
            egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSans-Regular.ttf"))
                .into(),
        );
        fonts.font_data.insert(
            "noto-bold".into(),
            egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSans-Bold.ttf")).into(),
        );
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "noto".into());
        let mut bold = fonts.families[&egui::FontFamily::Proportional].clone();
        bold.insert(0, "noto-bold".into());
        fonts
            .families
            .insert(egui::FontFamily::Name("bold".into()), bold);
        ctx.set_fonts(fonts);
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BACKGROUND;
        visuals.override_text_color = Some(TEXT);
        visuals.selection.bg_fill = MINT;
        visuals.selection.stroke = Stroke::new(1.5_f32, BACKGROUND);
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
        ] {
            widget.corner_radius = 8.into();
            widget.bg_stroke = Stroke::new(1.0_f32, BORDER);
            widget.fg_stroke = Stroke::new(1.5_f32, TEXT);
        }
        visuals.widgets.inactive.weak_bg_fill = SURFACE;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.5_f32, MINT);
        visuals.widgets.active.bg_stroke = Stroke::new(2.0_f32, MINT);
        ctx.set_visuals(visuals);
        ctx.style_mut(|style| {
            style.spacing.scroll = egui::style::ScrollStyle::solid();
            style.spacing.item_spacing = Vec2::new(10.0, 10.0);
            style.spacing.button_padding = Vec2::new(18.0, 10.0);
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(16.0));
        });
        let image = image::load_from_memory(include_bytes!(
            "../../assets/branding/infuser/header-light-640.png"
        ))
        .expect("embedded approved logo")
        .into_rgba8();
        let logo = ctx.load_texture(
            "infuser-brand",
            egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            ),
            egui::TextureOptions::LINEAR,
        );
        Self {
            logo,
            about_open: false,
            about_info: None,
            copied: false,
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        screen: Screen<'_>,
        update_info: impl FnOnce() -> UpdateInfo,
    ) -> Action {
        let mut action = Action::None;
        egui::TopBottomPanel::bottom("about-link")
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin::symmetric(24, 8)),
            )
            .show(ctx, |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("About Infuser Play").size(13.0).color(MUTED),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    self.about_open = true;
                    self.about_info = None;
                    self.copied = false;
                }
            });
        if matches!(
            screen,
            Screen::Connect { .. }
                | Screen::Error {
                    has_request: true,
                    ..
                }
        ) {
            egui::TopBottomPanel::bottom("actions")
                .show_separator_line(false)
                .frame(
                    egui::Frame::new()
                        .fill(BACKGROUND)
                        .inner_margin(egui::Margin {
                            left: 24,
                            right: 24,
                            top: 12,
                            bottom: 24,
                        }),
                )
                .show(ctx, |ui| {
                    if matches!(screen, Screen::Connect { .. }) {
                        if primary(ui, "Connect & sign in").clicked() {
                            action = Action::Connect;
                        }
                    } else {
                        if primary(ui, "Back to Library").clicked() {
                            action = Action::Library;
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Switch player / reconnect").color(MUTED),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            action = Action::Reconnect;
                        }
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(egui::Margin { left: 24, right: 24, top: 24, bottom: 4 }))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new(&self.logo).fit_to_exact_size(Vec2::new(208.0, 52.0))
                        .alt_text("Infuser"));
                    ui.label(RichText::new("Play").size(22.0).color(MUTED));
                });
                ui.add_space(12.0);
                let (line, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
                ui.painter().hline(line.x_range(), line.center().y, Stroke::new(1.0_f32, BORDER));
                ui.add_space(14.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    match screen {
                        Screen::Idle => {
                            heading(ui, "Ready to play");
                            ui.label(RichText::new("Choose a game in your Infuser Library.").color(MUTED));
                        }
                        Screen::Connect { server, note } => {
                            heading(ui, "Connect Infuser Play");
                            inset(ui, |ui| { ui.label(RichText::new(server).size(17.0)).on_hover_text(server); });
                            ui.label(RichText::new("Only connect to a server you trust. Its games can run on your computer.").color(MUTED));
                            if !note.is_empty() { ui.label(RichText::new(note).color(MUTED)); }
                        }
                        Screen::Busy(status) => {
                            heading(ui, status);
                            activity(ui);
                        }
                        Screen::Playing => {
                            heading(ui, "Game running");
                            ui.label(RichText::new("Infuser Play is keeping your game connected.").color(MUTED));
                        }
                        Screen::Error { message, has_request } => {
                            ui.horizontal_top(|ui| {
                                let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 28.0), egui::Sense::hover());
                                let amber = Color32::from_rgb(237, 190, 112);
                                let center = rect.center();
                                ui.painter().circle_stroke(center, 10.0, Stroke::new(1.5_f32, amber));
                                ui.painter().line_segment([center + Vec2::new(0.0, -5.0), center + Vec2::new(0.0, 1.0)], Stroke::new(1.5_f32, amber));
                                ui.painter().circle_filled(center + Vec2::new(0.0, 5.0), 1.0, amber);
                                heading(ui, if has_request { "Couldn’t open the game" } else { "Couldn’t open Infuser Play" });
                            });
                            inset(ui, |ui| { ui.label(message); });
                        }
                    }
                });
            });
        if self.about_open {
            if self
                .about_info
                .as_ref()
                .is_none_or(|(at, _)| at.elapsed() >= Duration::from_secs(1))
            {
                self.about_info = Some((Instant::now(), update_info()));
            }
            let info = &self.about_info.as_ref().unwrap().1;
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::new("about-infuser-play")).show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 64.0).clamp(200.0, 360.0));
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                heading(ui, "About Infuser Play");
                egui::ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() - 200.0).max(80.0))
                    .show(ui, |ui| {
                        ui.label(format!("Version {}", about::VERSION));
                        ui.label(format!(
                            "Build {}",
                            about::BUILD.get(..12).unwrap_or(about::BUILD)
                        ))
                        .on_hover_text(about::BUILD);
                        ui.label(
                            RichText::new(format!(
                                "{} · {}",
                                std::env::consts::OS,
                                std::env::consts::ARCH
                            ))
                            .color(MUTED),
                        );
                        ui.separator();
                        ui.label(info.status);
                        ui.label(RichText::new(info.last_check()).small().color(MUTED));
                        ui.label(
                            RichText::new("Updates are checked at launch, at most once an hour.")
                                .small()
                                .color(MUTED),
                        );
                    });
                if primary(
                    ui,
                    if self.copied {
                        "Copied!"
                    } else {
                        "Copy diagnostic info"
                    },
                )
                .clicked()
                {
                    ctx.copy_text(info.diagnostics());
                    self.copied = true;
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
            if close || modal.should_close() {
                self.about_open = false;
            }
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        action
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(22.0)
                .family(egui::FontFamily::Name("bold".into())),
        )
        .wrap(),
    );
    ui.add_space(4.0);
}

fn inset(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(8)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            contents(ui);
        });
}

fn primary(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .family(egui::FontFamily::Name("bold".into()))
                .color(BACKGROUND),
        )
        .fill(MINT)
        .min_size(Vec2::new(0.0, 42.0)),
    )
}

fn activity(ui: &mut egui::Ui) {
    ui.add_space(18.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(58.0), egui::Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, 12, SURFACE);
        let c = rect.center();
        let stroke = Stroke::new(2.5_f32, TEXT);
        p.line_segment([c + Vec2::new(0.0, -15.0), c + Vec2::new(0.0, 5.0)], stroke);
        p.line_segment([c + Vec2::new(-7.0, -2.0), c + Vec2::new(0.0, 5.0)], stroke);
        p.line_segment([c + Vec2::new(7.0, -2.0), c + Vec2::new(0.0, 5.0)], stroke);
        p.add(egui::Shape::line(
            vec![
                c + Vec2::new(-13.0, 7.0),
                c + Vec2::new(-13.0, 13.0),
                c + Vec2::new(13.0, 13.0),
                c + Vec2::new(13.0, 7.0),
            ],
            stroke,
        ));
    });
    ui.add_space(20.0);
    let (track, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), egui::Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, false, "In progress")
    });
    ui.painter().rect_filled(track, 3, SURFACE);
    // Indeterminate activity only: the client does not report download percentages.
    let phase = (ui.input(|i| i.time) * 1.6).sin() as f32 * 0.5 + 0.5;
    let width = track.width() * 0.22;
    let left = track.left() + phase * (track.width() - width);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(left, track.top()),
            Vec2::new(width, track.height()),
        ),
        3,
        MINT,
    );
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_opens_copies_running_build_and_closes_at_minimum_window_size() {
        let ctx = egui::Context::default();
        let mut presentation = Presentation::new(&ctx);
        let mut time = 0.0;
        let mut frame = |events| {
            time += 0.25;
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::splat(360.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    presentation.show(ctx, Screen::Idle, || UpdateInfo {
                        status: "Update check or download failed; using a working version",
                        checked: Some(123),
                    });
                },
            )
        };
        fn find(shape: &egui::epaint::Shape, text: &str) -> Option<egui::Pos2> {
            match shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == text => {
                    Some(t.pos + t.galley.size() * 0.5)
                }
                egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, text)),
                _ => None,
            }
        }
        fn position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
            let pos = output
                .shapes
                .iter()
                .find_map(|s| find(&s.shape, text))
                .expect(text);
            assert!(
                egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(360.0)).contains(pos),
                "{text} must be visible"
            );
            pos
        }
        let click = |pos, pressed| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        frame(vec![]);
        let idle = frame(vec![]);
        let about = position(&idle, "About Infuser Play");
        frame(click(about, true));
        frame(click(about, false));
        frame(vec![]);
        let opened = frame(vec![]);
        let copy = position(&opened, "Copy diagnostic info");
        frame(click(copy, true));
        let copied = frame(click(copy, false));
        let text = copied
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text),
                _ => None,
            })
            .expect("copy button must write the diagnostic payload");
        assert!(text.contains(&format!("Build: {}", about::BUILD)));
        assert!(text.contains("Update check or download failed"));
        let close = position(&frame(vec![]), "Close");
        frame(click(close, true));
        frame(click(close, false));
        frame(vec![]);
        assert!(!presentation.about_open);
    }
}
