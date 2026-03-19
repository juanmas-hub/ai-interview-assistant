pub mod deepgram;

use std::fs::File;
use std::io::Write;
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

pub async fn run(
    mut rx:  mpsc::Receiver<TurnComplete>,
    forward: mpsc::Sender<TurnComplete>,
) {
    let mut transcript = open_transcript();

    while let Some(turn) = rx.recv().await {
        if turn.text.trim().is_empty() { continue; }

        log_turn(&turn, &mut transcript);
        let _ = forward.send(turn).await;
    }
}

fn open_transcript() -> File {
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("transcript.txt")
        .expect("failed to open transcript.txt")
}

fn log_turn(turn: &TurnComplete, file: &mut File) {
    let line = format!("{}: {}\n", speaker_label(&turn.speaker), turn.text.trim());
    print!("{line}");
    if let Err(e) = file.write_all(line.as_bytes()) {
        eprintln!("[transcript] write error: {e}");
    }
}

fn speaker_label(speaker: &Speaker) -> &'static str {
    match speaker {
        Speaker::User   => "[User]",
        Speaker::System => "[Interviewer]",
    }
}