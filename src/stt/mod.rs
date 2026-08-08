pub mod deepgram;

use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::audio::Speaker;
use crate::transcript::{LineKind, Transcript};

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
    transcript.lock().unwrap().log(line_kind_for(turn.speaker), &turn.text);
}

fn line_kind_for(speaker: Speaker) -> LineKind {
    match speaker {
        Speaker::User   => LineKind::User,
        Speaker::System => LineKind::Interviewer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc;
    use mockall::mock;

    mock! {
        pub TestSttSender {}
        impl SttSender for TestSttSender {
            fn send_audio(&self, samples: &[i16]);
            fn end_turn(&self);
        }
    }

    fn temp_transcript_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("ai_interview_assistant_stt_{unique}.txt"))
    }

    #[test]
    fn line_kind_for_maps_each_speaker() {
        assert_eq!(line_kind_for(Speaker::User), LineKind::User);
        assert_eq!(line_kind_for(Speaker::System), LineKind::Interviewer);
    }

    #[test]
    fn log_turn_writes_formatted_line_to_transcript_file() {
        let path = temp_transcript_path();
        let transcript = Arc::new(Mutex::new(Transcript::open(
            path.to_str().unwrap(),
            crate::transcript::new_live_transcript(),
        )));
        let turn = TurnComplete {
            speaker: Speaker::User,
            text: "Hello from tests".to_string(),
        };

        log_turn(&turn, &transcript);

        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(written, "[User]: Hello from tests\n");

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn run_forwards_non_empty_turns_and_writes_transcript() {
        let path = temp_transcript_path();
        let transcript = Arc::new(Mutex::new(Transcript::open(
            path.to_str().unwrap(),
            crate::transcript::new_live_transcript(),
        )));
        let (tx, rx) = mpsc::channel(4);
        let (forward_tx, mut forward_rx) = mpsc::channel(4);

        tokio::spawn(run(rx, forward_tx, transcript.clone()));

        tx.send(TurnComplete {
            speaker: Speaker::System,
            text: "  Hello world  ".to_string(),
        })
        .await
        .unwrap();
        drop(tx);

        let received = forward_rx.recv().await.unwrap();
        assert_eq!(received.text, "  Hello world  ");
        assert_eq!(received.text.trim(), "Hello world");

        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("[Interviewer]: Hello world"));

        let _ = fs::remove_file(path);
    }
}