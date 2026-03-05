mod config;
mod audio;
mod stt;
mod ai;
mod ui;

use tokio::sync::mpsc;
use anyhow::Result;

// contracts
pub enum AudioEvent {
    Chunk { is_user: bool, data: Vec<f32>, sample_rate: u32 },
    Error(String),
}

pub enum TextEvent {
    SttTranscription { speaker: String, text: String },
    LlmToken { token: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("AI Interview Copilot...");

    // Channels
    let channel_capacity: usize = 100;
    let (audio_transmitter, audio_reciever) = mpsc::channel::<AudioEvent>(channel_capacity);
    let (ui_transmitter, mut ui_reciever) = mpsc::channel::<TextEvent>(channel_capacity);
    
    let ui_transmitter_for_stt = ui_transmitter.clone();
    let ui_transmitter_for_llm = ui_transmitter;

    // Audio
    std::thread::spawn(move || {
        // src/audio/wasapi.rs
        // y enviaremos los bytes por audio_tx
    });

    // STT
    tokio::spawn(async move {
        // Escucha audio_reciever, envía a Deepgram, y manda el texto por ui_transmitter_for_stt
    });

    // AI
    tokio::spawn(async move {
        // Lee transcripciones, consulta la DB Vectorial, llama al LLM,
        // y manda los tokens de respuesta por ui_transmitter_for_llm
    });

    // UI Overlay
    // ui::renderer::start_overlay(ui_reciever)?;

    tokio::signal::ctrl_c().await?;
    Ok(())
}