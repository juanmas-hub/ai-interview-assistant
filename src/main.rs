mod audio;
mod stt;
mod pipeline;
mod ai;
mod ui;
mod setup;
mod config;

use anyhow::Result;
use config::Environment;

#[tokio::main]
async fn main() -> Result<()> {
    println!("AI Interview Copilot starting…");

    let env = Environment::load();
    env.start_hotkey_listener();
    pipeline::start(env).await?;

    tokio::signal::ctrl_c().await?;
    println!("Shutting down…");
    Ok(())
}