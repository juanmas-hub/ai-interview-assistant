mod config;
mod audio;
mod stt;
mod ai;
mod ui;

use tokio::sync::mpsc;
use anyhow::Result;
use audio::wav_writer::DualWavWriter;

pub enum AudioEvent {
    Chunk { is_user: bool, data: Vec<f32>, sample_rate: u32, channels: u16 },
    ResampledChunk { is_user: bool, data: Vec<i16> },
    Error(String),
}

/*pub enum TextEvent {
    SttTranscription { speaker: String, text: String },
    LlmToken { token: String },
}*/

#[tokio::main]
async fn main() -> Result<()> {
    println!("AI Interview Copilot");

    let (audio_transmitter, mut audio_reader) = mpsc::channel::<AudioEvent>(1000);
    //let (ui_tx, ui_rx)           = mpsc::channel::<TextEvent>(100);

    // Audio capture
    audio::wasapi::start_concurrent_capture(audio_transmitter)?;

    // WAV writer — for testing
    tokio::spawn(async move {
        let mut wav = DualWavWriter::new();

        while let Some(event) = audio_reader.recv().await {
            match event {
                AudioEvent::Chunk { is_user, data, sample_rate, channels } =>
                    wav.write_chunk(is_user, &data, sample_rate, channels),

                AudioEvent::ResampledChunk { .. } => {}

                AudioEvent::Error(e) =>
                    eprintln!("[audio] Stream error: {}", e),
            }
        }
    });

    // STT
    // stt::

    // AI
    // ai::

    // UI Overlay
    // ui::renderer::start_overlay(ui_r)?;

    tokio::signal::ctrl_c().await?;
    println!("Cerrando...");
    Ok(())
}