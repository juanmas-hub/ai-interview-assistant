pub mod deepgram;

use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::audio::Speaker;
use crate::transcript::Transcript;

pub struct TurnComplete {
    pub speaker: Speaker,
    pub text:    String,
}

pub trait SttSender: Send + Sync + 'static {
    fn send_audio(&self, samples: &[i16]);
    fn end_turn(&self);
}

pub async fn run(
    mut rx:     mpsc::Receiver<TurnComplete>,
    forward:    mpsc::Sender<TurnComplete>,
    transcript: Arc<Mutex<Transcript>>,
) {
    while let Some(turn) = rx.recv().await {
        if turn.text.trim().is_empty() { continue; }

        log_turn(&turn, &transcript);
        let _ = forward.send(turn).await;
    }
}

fn log_turn(turn: &TurnComplete, transcript: &Arc<Mutex<Transcript>>) {
    let line = format!("{}: {}\n", speaker_label(&turn.speaker), turn.text.trim());
    transcript.lock().unwrap().write_line(&line);
}

fn speaker_label(speaker: &Speaker) -> &'static str {
    match speaker {
        Speaker::User   => "[User]",
        Speaker::System => "[Interviewer]",
    }
}