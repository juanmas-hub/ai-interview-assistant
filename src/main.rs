mod config;
mod audio;
mod stt;
mod ai;
mod ui;

use tokio::sync::mpsc;
use anyhow::Result;

use audio::AudioEvent;
use audio::hotkey;

#[tokio::main]
async fn main() -> Result<()> {
    println!("AI Interview Copilot starting…");

    let pause_flag = hotkey::new_pause_flag();
    hotkey::spawn_hotkey_listener(pause_flag.clone());

    let (audio_tx, audio_rx) = mpsc::channel::<AudioEvent>(1_000);

    audio::wasapi::start_concurrent_capture(audio_tx)?;
    tokio::spawn(audio::pipeline::run(audio_rx, pause_flag));

    tokio::signal::ctrl_c().await?;
    println!("Shutting down…");
    Ok(())
}