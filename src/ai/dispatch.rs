use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::ai::RagEngine;
use crate::audio::Speaker;
use crate::stt::TurnComplete;
use crate::transcript::Transcript;

pub async fn run_ai(
    mut rx:     mpsc::Receiver<TurnComplete>,
    rag_engine: Arc<RagEngine>,
    transcript: Arc<Mutex<Transcript>>,
) {
    while let Some(turn) = rx.recv().await {
        if !is_interviewer_question(&turn) { continue; }
        answer_in_background(turn, rag_engine.clone(), transcript.clone());
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn is_interviewer_question(turn: &TurnComplete) -> bool {
    turn.speaker == Speaker::System
}

fn answer_in_background(
    turn:       TurnComplete,
    rag_engine: Arc<RagEngine>,
    transcript: Arc<Mutex<Transcript>>,
) {
    tokio::spawn(async move {
        match rag_engine.answer(&turn.text).await {
            Ok(response) => output_ai_response(&response, &transcript),
            Err(e)       => eprintln!("[ai] error: {e}"),
        }
    });
}

fn output_ai_response(response: &str, transcript: &Arc<Mutex<Transcript>>) {
    let line = format!("[AI] {response}\n");
    transcript.lock().unwrap().write_line(&line);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_transcript_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("ai_dispatch_{unique}.txt"))
    }

    #[test]
    fn is_interviewer_question_only_matches_system_speaker() {
        let user_turn = TurnComplete {
            speaker: Speaker::User,
            text: "Hello".to_string(),
        };
        let system_turn = TurnComplete {
            speaker: Speaker::System,
            text: "What is Rust?".to_string(),
        };

        assert!(!is_interviewer_question(&user_turn));
        assert!(is_interviewer_question(&system_turn));
    }

    #[test]
    fn output_ai_response_writes_prefixed_line_to_transcript() {
        let path = temp_transcript_path();
        let transcript = Arc::new(Mutex::new(Transcript::open(path.to_str().unwrap())));

        output_ai_response("hello", &transcript);

        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(written, "[AI] hello\n");

        let _ = fs::remove_file(path);
    }
}