mod config;
mod audio;
mod stt;
mod pipeline;
mod ai;
mod ui;

use anyhow::Result;
use config::Environment;

#[tokio::main]
async fn main() -> Result<()> {
    println!("AI Interview Copilot starting…");

    let env = Environment::load();
    pipeline::start(env).await?;

    tokio::signal::ctrl_c().await?;
    println!("Shutting down…");
    Ok(())
}