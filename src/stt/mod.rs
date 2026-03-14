pub mod deepgram;

use tokio::sync::mpsc;

use crate::audio::Speaker;

#[derive(Debug)]
pub struct TranscriptEvent {
    pub speaker:      Speaker,
    pub text:         String,
    // pub is_final:     bool,
    pub speech_final: bool,
}

pub trait SttSender: Send + 'static {
    fn send(&self, samples: &[i16]);
}

pub async fn run(mut rx: mpsc::Receiver<TranscriptEvent>) {
    while let Some(ev) = rx.recv().await {
        if ev.speech_final && !ev.text.trim().is_empty() {
            let label = match ev.speaker {
                Speaker::User   => "[User]",
                Speaker::System => "[Interviewer]",
            };
            println!("{label}: {}", ev.text.trim());
        }
    }
}