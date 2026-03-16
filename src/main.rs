mod config;
mod audio;
mod stt;
mod pipeline;
mod ai;
mod ui;

use anyhow::Result;
use tokio::sync::mpsc;
use dotenvy::dotenv;

use audio::{AudioEvent};
use audio::hotkey;
use stt::deepgram::DeepgramSender;

#[tokio::main]
async fn main() -> Result<()> {
    println!("AI Interview Copilot starting…");
    dotenv().ok();

    let api_key = std::env::var("DEEPGRAM_API_KEY")
        .expect("DEEPGRAM_API_KEY env var not set");

    let pause_flag = hotkey::new_pause_flag();
    hotkey::spawn_hotkey_listener(pause_flag.clone());

    let (audio_tx, audio_rx) = mpsc::channel::<AudioEvent>(1_000);

    let user_stt:   Box<dyn stt::SttSender> = Box::new(DeepgramSender::new(&api_key));
    let system_stt: Box<dyn stt::SttSender> = Box::new(DeepgramSender::new(&api_key));

    audio::wasapi::start_concurrent_capture(audio_tx)?;
    tokio::spawn(pipeline::run(audio_rx, pause_flag, user_stt, system_stt));

    tokio::signal::ctrl_c().await?;
    println!("Shutting down…");
    Ok(())
}