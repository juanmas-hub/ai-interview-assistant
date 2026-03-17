pub mod deepgram;

use tokio::sync::mpsc;
use std::io::Write;
use crate::audio::Speaker;

pub struct TurnComplete {
    pub speaker: Speaker,
    pub text:    String,
}

pub trait SttSender: Send + Sync + 'static {
    fn send_audio(&self, samples: &[i16]);
    fn end_turn(&self);
}
pub async fn run(
    mut rx:  mpsc::Receiver<TurnComplete>,
    forward: mpsc::Sender<TurnComplete>,
) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("transcript.txt")
        .expect("failed to open transcript.txt");

    while let Some(turn) = rx.recv().await {
        if turn.text.trim().is_empty() { continue; }

        let line = format!("{}: {}\n", label(&turn.speaker), turn.text.trim());
        print!("{line}");

        if let Err(e) = file.write_all(line.as_bytes()) {
            eprintln!("[transcript] write error: {e}");
        }

        let _ = forward.send(turn).await;
    }
}

fn label(speaker: &Speaker) -> &'static str {
    match speaker {
        Speaker::User   => "[User]",
        Speaker::System => "[Interviewer]",
    }
}