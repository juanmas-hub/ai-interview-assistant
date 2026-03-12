mod config;
mod audio;
mod stt;
mod ai;
mod ui;

use tokio::sync::mpsc;
use anyhow::Result;

use audio::{AudioEvent, Speaker};
use audio::vad::{SpeechTurn, VadChannel};
use audio::wav_writer::{CaptureRecorder, save_turn_as_wav};
use audio::hotkey;

#[tokio::main]
async fn main() -> Result<()> {
    println!("AI Interview Copilot starting…");

    let pause_flag = hotkey::new_pause_flag();
    hotkey::spawn_hotkey_listener(pause_flag.clone());  

    let (audio_tx, audio_rx) = mpsc::channel::<AudioEvent>(1_000);

    audio::wasapi::start_concurrent_capture(audio_tx, pause_flag)?;

    tokio::spawn(process_audio_stream(audio_rx));

    tokio::signal::ctrl_c().await?;
    println!("Shutting down…");
    Ok(())
}

async fn process_audio_stream(mut rx: mpsc::Receiver<AudioEvent>) {
    let mut user_vad = VadChannel::new(Speaker::User)
        .expect("failed to initialise user (microphone) VAD channel");
    let mut system_vad = VadChannel::new(Speaker::System)
        .expect("failed to initialise system (loopback) VAD channel");

    let mut recorder     = CaptureRecorder::new();
    let mut conversation: Vec<SpeechTurn> = Vec::new();

    while let Some(event) = rx.recv().await {
        match event {
            AudioEvent::RawCapture { speaker, samples, format } => {
                recorder.record_chunk(speaker, &samples, format);
            }

            AudioEvent::NormalizedCapture { speaker, samples } => {
                let new_turns = match speaker {
                    Speaker::User   => user_vad.push(&samples),
                    Speaker::System => system_vad.push(&samples),
                };

                for turn in new_turns {
                    on_turn_completed(&turn);
                    insert_chronologically(&mut conversation, turn);
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