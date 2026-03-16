pub mod deepgram;

use tokio::sync::mpsc;
use crate::audio::Speaker;

pub struct TurnComplete {
    pub speaker: Speaker,
    pub text:    String,
}

pub trait SttSender: Send + Sync + 'static {
    fn send_audio(&self, samples: &[i16]);
    fn end_turn(&self);
}

pub async fn run(mut rx: mpsc::Receiver<TurnComplete>) {
    while let Some(turn) = rx.recv().await {
        if turn.text.trim().is_empty() { continue; }

        let label = match turn.speaker {
            Speaker::User   => "[User]",
            Speaker::System => "[Interviewer]",
        };
        println!("{label}: {}", turn.text.trim());

    }
}