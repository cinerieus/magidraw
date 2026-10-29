#![cfg_attr(windows, windows_subsystem = "windows")]

mod contrast;
mod drawing;
mod edges;
mod platform;
mod timing;

use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::Duration;

use drawing::{Mode, Plan, Settings};
use eframe::egui;
use image::DynamicImage;

struct App {
    path: Option<PathBuf>,
    image: Option<DynamicImage>,
    plan: Option<Plan>,
    texture: Option<egui::TextureHandle>,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    step: u32,
    threshold: u8,
    balance: u8,
    detail: u8,
    smoothing: u8,
    fill_enclosed: bool,
    fill_spacing: u32,
    mode: Mode,
    invert: bool,
    delay_ms: u64,
    dark_delay_ms: u64,
    button_settle_ms: u64,
    pen_lift_ms: u64,
    speed: f32,
    countdown: u64,
    status: String,
    receiver: Option<mpsc::Receiver<String>>,
    cancel: Option<Arc<AtomicBool>>,
    worker: Option<thread::JoinHandle<()>>,
    f6_held: bool,
    f7_held: bool,
    f8_held: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            path: None,
            image: None,
            plan: None,
            texture: None,
            left: 200,
            top: 200,
            right: 1000,
            bottom: 800,
            step: 3,
            threshold: 90,
            balance: 35,
            detail: 15,
            smoothing: 1,
            fill_enclosed: false,
            fill_spacing: 2,
            mode: Mode::Edges,
            invert: false,
            delay_ms: 12,
            dark_delay_ms: 2,
            button_settle_ms: 35,
            pen_lift_ms: 60,
            speed: 1.0,
            countdown: 5,
            status: "Load an image to begin".into(),
            receiver: None,
            cancel: None,
            worker: None,
            f6_held: false,
            f7_held: false,
            f8_held: false,
        }
    }
}

impl App {
    fn load(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
            .pick_file()
        else {
            return;
        };
        match image::open(&path) {
            Ok(image) => {
                self.path = Some(path);
                self.image = Some(image);
                self.status = "Image loaded".into();
                self.plan = None;
            }
            Err(error) => self.status = format!("Could not open image: {error}"),
        }
    }

    fn rebuild(&mut self, ctx: &egui::Context) {
        let (Some(image), Some(width), Some(height)) = (
            self.image.as_ref(),
            self.right.checked_sub(self.left).filter(|v| *v > 0),
            self.bottom.checked_sub(self.top).filter(|v| *v > 0),
        ) else {
            self.plan = None;
            self.texture = None;
            self.status = "Set bottom-right below and to the right of top-left".into();
            return;
        };
        if width > 4096 || height > 4096 {
            self.plan = None;
            self.texture = None;
            self.status = "Whiteboard bounds cannot exceed 4096 pixels per side".into();
            return;
        }
        let settings = Settings {
            width: width as u32,
            height: height as u32,
            step: self.step,
            threshold: self.threshold,
            balance: self.balance,
            detail: self.detail,
            smoothing: self.smoothing,
            fill_enclosed: self.fill_enclosed,
            fill_spacing: self.fill_spacing,
            mode: self.mode,
            invert: self.invert,
        };
        let plan = drawing::plan(image, settings);
        self.texture = Some(ctx.load_texture(
            "preview",
            plan.preview.clone(),
            egui::TextureOptions::NEAREST,
        ));
        self.status = if self.mode == Mode::Edges && self.fill_enclosed {
            format!(
                "Ready: {} strokes ({} joins, {} fill paths)",
                plan.strokes.len(),
                plan.joined_strokes,
                plan.fill_strokes
            )
        } else {
            format!(
                "Ready: {} strokes ({} joins)",
                plan.strokes.len(),
                plan.joined_strokes
            )
        };
        self.plan = Some(plan);
    }

    fn hotkeys(&mut self) -> bool {
        let mut changed = false;
        let f6 = platform::f6();
        let f7 = platform::f7();
        let f8 = platform::f8();
        if f6
            && !self.f6_held
            && let Some((x, y)) = platform::cursor()
        {
            self.left = x;
            self.top = y;
            changed = true;
        }
        if f7
            && !self.f7_held
            && let Some((x, y)) = platform::cursor()
        {
            self.right = x;
            self.bottom = y;
            changed = true;
        }
        self.f6_held = f6;
        self.f7_held = f7;
        if f8 && !self.f8_held && self.cancel.is_none() && !changed {
            self.start();
        }
        self.f8_held = f8;
        changed
    }

    fn start(&mut self) {
        let Some(plan) = self.plan.take() else { return };
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let job = platform::DrawJob {
            plan,
            left: self.left,
            top: self.top,
            width: (self.right - self.left) as u32,
            height: (self.bottom - self.top) as u32,
            delay_ms: if self.mode == Mode::Dark {
                self.dark_delay_ms
            } else {
                self.delay_ms
            },
            button_settle_ms: self.button_settle_ms,
            pen_lift_ms: self.pen_lift_ms,
            speed: self.speed,
            countdown: self.countdown,
            cancel: Arc::clone(&cancel),
            updates: sender,
        };
        self.cancel = Some(cancel);
        self.receiver = Some(receiver);
        self.status = "Preparing drawing".into();
        self.worker = Some(thread::spawn(move || platform::run(job)));
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut changed = self.hotkeys();
        let mut finished = false;
        if let Some(receiver) = &self.receiver {
            while let Ok(message) = receiver.try_recv() {
                finished = message == "Done"
                    || message == "Stopped"
                    || message.starts_with("SendInput")
                    || message.starts_with("Invalid")
                    || message == "Drawing requires Windows";
                self.status = message;
            }
            if matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Disconnected)) {
                finished = true;
            }
        }
        if finished {
            self.receiver = None;
            self.cancel = None;
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            changed = true;
        }

        {
            ui.heading("MagiDraw");
            ui.horizontal(|ui| {
                if ui.button("Open image").clicked() {
                    self.load();
                    changed = true;
                }
                if let Some(path) = &self.path {
                    ui.label(path.display().to_string());
                }
            });
            ui.separator();
            ui.label("Whiteboard bounds: hover over top-left and press F6; hover over bottom-right and press F7.");
            ui.horizontal(|ui| {
                ui.label("Left");
                changed |= ui.add(egui::DragValue::new(&mut self.left)).changed();
                ui.label("Top");
                changed |= ui.add(egui::DragValue::new(&mut self.top)).changed();
                ui.label("Right");
                changed |= ui.add(egui::DragValue::new(&mut self.right)).changed();
                ui.label("Bottom");
                changed |= ui.add(egui::DragValue::new(&mut self.bottom)).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Mode");
                changed |= ui
                    .selectable_value(&mut self.mode, Mode::Edges, "Edges")
                    .changed();
                changed |= ui
                    .selectable_value(&mut self.mode, Mode::LineArt, "Line art")
                    .changed();
                changed |= ui
                    .selectable_value(&mut self.mode, Mode::Dark, "Dark areas")
                    .changed();
                changed |= ui.checkbox(&mut self.invert, "Invert dark areas").changed();
            });
            ui.horizontal(|ui| {
                ui.label(if self.mode == Mode::Edges {
                    "Edge strength"
                } else {
                    "Dark cutoff"
                });
                changed |= ui
                    .add(egui::Slider::new(&mut self.threshold, 1..=254))
                    .changed();
                ui.label("Point spacing");
                changed |= ui.add(egui::Slider::new(&mut self.step, 2..=16)).changed();
                if self.mode == Mode::Edges {
                    ui.label("Smooth");
                    changed |= ui
                        .add(egui::Slider::new(&mut self.smoothing, 0..=3))
                        .changed();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Local balance");
                changed |= ui
                    .add(egui::Slider::new(&mut self.balance, 0..=100).suffix("%"))
                    .changed();
                ui.label("0% keeps original tones; higher values reveal local detail");
            });
            if self.mode != Mode::Edges {
                ui.horizontal(|ui| {
                    ui.label("Local detail difference");
                    changed |= ui
                        .add(egui::Slider::new(&mut self.detail, 0..=50))
                        .changed();
                    ui.label("0 turns off; lower values find fainter marks");
                });
            }
            if self.mode == Mode::Edges {
                ui.horizontal(|ui| {
                    changed |= ui
                        .checkbox(&mut self.fill_enclosed, "Fill enclosed areas")
                        .changed();
                    if self.fill_enclosed {
                        ui.label("Fill row spacing (px)");
                        changed |= ui
                            .add(egui::Slider::new(&mut self.fill_spacing, 1..=16))
                            .changed();
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.label("Speed");
                ui.add_enabled(
                    self.cancel.is_none(),
                    egui::Slider::new(&mut self.speed, 0.5..=20.0)
                        .logarithmic(true)
                        .suffix("×"),
                );
                ui.label("1× uses the current base timing");
            });
            let base_move = if self.mode == Mode::Dark {
                self.dark_delay_ms
            } else {
                self.delay_ms
            };
            ui.label(format!(
                "Effective timing: move {} ms, button {} ms, pen up {} ms",
                timing::scaled_ms(base_move, self.speed, 0),
                timing::scaled_ms(self.button_settle_ms, self.speed, 1),
                timing::scaled_ms(self.pen_lift_ms, self.speed, 1),
            ));
            ui.collapsing("Advanced timing", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Base move delay (ms)");
                    if self.mode == Mode::Dark {
                        ui.add(egui::Slider::new(&mut self.dark_delay_ms, 1..=50));
                    } else {
                        ui.add(egui::Slider::new(&mut self.delay_ms, 1..=50));
                    }
                    ui.label("Button settle (ms)");
                    ui.add(egui::Slider::new(&mut self.button_settle_ms, 10..=150));
                });
                ui.horizontal(|ui| {
                    ui.label("Pen-up pause (ms)");
                    ui.add(egui::Slider::new(&mut self.pen_lift_ms, 10..=200));
                });
            });
            ui.horizontal(|ui| {
                ui.label("Countdown (s)");
                ui.add(egui::Slider::new(&mut self.countdown, 1..=15));
            });
            ui.horizontal(|ui| {
                if self.cancel.is_none() {
                    if ui
                        .add_enabled(self.plan.is_some(), egui::Button::new("Start drawing"))
                        .clicked()
                    {
                        self.start();
                    }
                } else if ui.button("Stop drawing").clicked()
                    && let Some(cancel) = &self.cancel
                {
                    cancel.store(true, Ordering::Relaxed);
                }
                ui.label("F8 starts drawing. Esc stops and releases left click.");
            });
            ui.label(&self.status);
            if let Some(texture) = &self.texture {
                ui.separator();
                ui.label("Preview (black pixels will be drawn)");
                let size = texture.size_vec2();
                let scale = (ui.available_width() / size.x).min(400.0 / size.y);
                ui.image((texture.id(), size * scale));
            }
        }
        if changed && self.cancel.is_none() {
            let result = finished.then(|| self.status.clone());
            self.rebuild(&ctx);
            if let Some(result) = result {
                self.status = result;
            }
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([860.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MagiDraw",
        options,
        Box::new(|_| Ok(Box::new(App::default()))),
    )
}
