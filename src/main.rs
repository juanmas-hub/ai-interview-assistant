mod config;
mod audio;
mod stt;
mod pipeline;
mod ai;
mod ui;

use anyhow::Result;
use tokio::sync::mpsc;
use dotenvy::dotenv;

use audio::{AudioEvent, Speaker};
use audio::hotkey;
use stt::TranscriptEvent;
use stt::deepgram::DeepgramSender;

#[tokio::main]
async fn main() -> Result<()> {
    println!("AI Interview Copilot starting…");

    dotenv().ok();

    let api_key = std::env::var("DEEPGRAM_API_KEY")
        .expect("DEEPGRAM_API_KEY env var not set");

    let pause_flag = hotkey::new_pause_flag();
    hotkey::spawn_hotkey_listener(pause_flag.clone());

    let (audio_tx,      audio_rx)      = mpsc::channel::<AudioEvent>(1_000);
    let (transcript_tx, transcript_rx) = mpsc::channel::<TranscriptEvent>(256);

    let user_stt   = Box::new(DeepgramSender::connect(Speaker::User,   transcript_tx.clone(), &api_key).await?);
    let system_stt = Box::new(DeepgramSender::connect(Speaker::System, transcript_tx,         &api_key).await?);

    audio::wasapi::start_concurrent_capture(audio_tx)?;
    tokio::spawn(pipeline::run(audio_rx, pause_flag, user_stt, system_stt));
    tokio::spawn(stt::run(transcript_rx));

    tokio::signal::ctrl_c().await?;
    println!("Shutting down…");
    Ok(())
}