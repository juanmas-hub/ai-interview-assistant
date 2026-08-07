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