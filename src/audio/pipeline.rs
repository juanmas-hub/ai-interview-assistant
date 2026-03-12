use tokio::sync::mpsc;
use std::sync::atomic::Ordering;

use crate::audio::{AudioEvent, Speaker};
use crate::audio::hotkey::PauseFlag;
use crate::audio::vad::{SpeechTurn, VadChannel};
use crate::audio::wav_writer::{CaptureRecorder, save_turn_as_wav};
use super::normalizer::AudioNormalizer;

pub async fn run(mut rx: mpsc::Receiver<AudioEvent>, pause_flag: PauseFlag) {
    let mut user_vad   = VadChannel::new(Speaker::User)
        .expect("failed to initialise user VAD channel");
    let mut system_vad = VadChannel::new(Speaker::System)
        .expect("failed to initialise system VAD channel");

    let mut user_normalizer   = AudioNormalizer::new();
    let mut system_normalizer = AudioNormalizer::new();

    let mut recorder    = CaptureRecorder::new();
    let mut conversation: Vec<SpeechTurn> = Vec::new();

    while let Some(event) = rx.recv().await {

        if pause_flag.load(Ordering::Relaxed) {
            continue;
        }

        match event {
            AudioEvent::RawCapture { speaker, samples, format } => {
                recorder.record_chunk(speaker, &samples, format);

                let normalizer = match speaker {
                    Speaker::User   => &mut user_normalizer,
                    Speaker::System => &mut system_normalizer,
                };

                match normalizer.process(&samples, format) {
                    Ok(normalized) if !normalized.is_empty() => {
                        let new_turns = match speaker {
                            Speaker::User   => user_vad.push(&normalized),
                            Speaker::System => system_vad.push(&normalized),
                        };
                        for turn in new_turns {
                            on_turn_completed(&turn);
                            insert_chronologically(&mut conversation, turn);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("[normalizer] {speaker} error: {e}"),
                }
            }

            AudioEvent::CaptureError { speaker, error } => {
                eprintln!("[audio] {speaker} capture error: {error}");
            }
        }
    }
}

fn on_turn_completed(turn: &SpeechTurn) {
    println!("[TURN] {turn}");
    if let Err(e) = save_turn_as_wav(turn.speaker, turn.start_ms, &turn.audio) {
        eprintln!("[wav] Failed to save turn: {e}");
    }
}

fn insert_chronologically(conversation: &mut Vec<SpeechTurn>, turn: SpeechTurn) {
    let pos = conversation.partition_point(|t| t.start_ms <= turn.start_ms);
    conversation.insert(pos, turn);
}