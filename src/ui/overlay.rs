use std::sync::atomic::Ordering;
use std::time::Duration;

use eframe::egui;
use tokio::sync::oneshot;

use crate::audio::hotkey::PauseFlag;
use crate::transcript::{LineKind, LiveTranscript};

const AI_LINE_COLOR: egui::Color32 = egui::Color32::from_rgb(122, 162, 247);

pub fn run_window(
    pause_flag:      PauseFlag,
    live_transcript: LiveTranscript,
    context_tx:      oneshot::Sender<String>,
) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 480.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "AI Interview Copilot",
        options,
        Box::new(|cc| {
            cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::ContentProtected(true));

            Ok(Box::new(OverlayApp::new(pause_flag, live_transcript, context_tx)))
        }),
    );

    if let Err(err) = result {
        eprintln!("[ui] egui error: {err}");
    }
}

struct OverlayApp {
    pause_flag:      PauseFlag,
    live_transcript: LiveTranscript,
    context:         String,
    context_tx:      Option<oneshot::Sender<String>>,
    started:         bool,
    is_hidden:       bool,
}

impl OverlayApp {
    fn new(pause_flag: PauseFlag, live_transcript: LiveTranscript, context_tx: oneshot::Sender<String>) -> Self {
        Self {
            pause_flag,
            live_transcript,
            context:    String::new(),
            context_tx: Some(context_tx),
            started:    false,
            is_hidden:  true,
        }
    }

    fn start_session(&mut self) {
        if let Some(tx) = self.context_tx.take() {
            let _ = tx.send(self.context.clone());
            self.started = true;
        }
    }

    fn toggle_pause(&self) {
        self.pause_flag.fetch_xor(true, Ordering::Relaxed);
    }

    fn pause_label(&self) -> &'static str {
        if self.pause_flag.load(Ordering::Relaxed) { "Resume" } else { "Pause" }
    }
}

impl eframe::App for OverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("AI Interview Copilot");

            if self.started {
                self.show_session(ui);
            } else {
                self.show_setup(ui);
            }
        });

        ctx.request_repaint_after(Duration::from_millis(300));
    }
}

impl OverlayApp {
    fn show_setup(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let visibility_label = if self.is_hidden { "Unhide" } else { "Hide" };
            if ui.button(visibility_label).clicked() {
                self.is_hidden = !self.is_hidden;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::ContentProtected(self.is_hidden));
            }
        });
        ui.add_space(8.0);

        ui.label("Enter the candidate's initial context and press Start to begin.");
        ui.add_space(8.0);

        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.context)
                    .desired_width(f32::INFINITY)
                    .desired_rows(18)
                    .hint_text("Example: Senior Backend Developer with 5 years of experience using Node.js and PostgreSQL"),
            );
        });

        ui.add_space(12.0);
        if ui.button("Start session").clicked() {
            self.start_session();
        }
    }

    fn show_session(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button(self.pause_label()).clicked() {
                self.toggle_pause();
            }

            let visibility_label = if self.is_hidden { "Unhide" } else { "Hide" };
            if ui.button(visibility_label).clicked() {
                self.is_hidden = !self.is_hidden;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::ContentProtected(self.is_hidden));
            }

            if ui.button("Close session").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.label("Conversation preview");

        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            let lines = self.live_transcript.lock().unwrap();
            if lines.is_empty() {
                ui.colored_label(egui::Color32::GRAY, "Waiting for the conversation...");
            } else {
                for line in lines.iter() {
                    match line.kind {
                        LineKind::Ai => { ui.colored_label(AI_LINE_COLOR, line.to_string()); }
                        _            => { ui.label(line.to_string()); }
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn test_app() -> OverlayApp {
        let (tx, _rx) = oneshot::channel();
        OverlayApp::new(Arc::new(AtomicBool::new(false)), Arc::new(std::sync::Mutex::new(Vec::new())), tx)
    }

    #[test]
    fn pause_label_reflects_flag_state() {
        let app = test_app();
        assert_eq!(app.pause_label(), "Pause");

        app.toggle_pause();
        assert_eq!(app.pause_label(), "Resume");
    }

    #[test]
    fn start_session_sends_context_exactly_once() {
        let (tx, rx) = oneshot::channel();
        let mut app = OverlayApp::new(Arc::new(AtomicBool::new(false)), Arc::new(std::sync::Mutex::new(Vec::new())), tx);
        app.context = "candidate background".to_string();

        app.start_session();

        assert!(app.started);
        assert!(app.context_tx.is_none());
        assert_eq!(rx.blocking_recv().unwrap(), "candidate background");
    }
}