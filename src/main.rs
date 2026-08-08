mod audio;
mod stt;
mod pipeline;
mod ai;
mod ui;
mod config;
mod transcript;

use anyhow::Result;
use config::Environment;


fn main() -> Result<()> {
    println!("AI Interview Copilot starting…");

    let env = Environment::load();
    env.start_hotkey_listener();

    let pause_flag                = env.pause_flag.clone();
    let live_transcript           = transcript::new_live_transcript();
    let (context_tx, context_rx)  = tokio::sync::oneshot::channel();

    spawn_pipeline(env, live_transcript.clone(), context_rx);

    ui::run_blocking(pause_flag, live_transcript, context_tx);

    Ok(())
}

fn spawn_pipeline(
    env:             Environment,
    live_transcript: transcript::LiveTranscript,
    context_rx:      tokio::sync::oneshot::Receiver<String>,
) {
    std::thread::Builder::new()
        .name("pipeline-runtime".into())
        .spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
            rt.block_on(run_pipeline(env, live_transcript, context_rx));
        })
        .expect("failed to spawn pipeline-runtime thread");
}

async fn run_pipeline(
    env:             Environment,
    live_transcript: transcript::LiveTranscript,
    context_rx:      tokio::sync::oneshot::Receiver<String>,
) {
    if let Err(e) = pipeline::start(env, live_transcript, context_rx).await {
        eprintln!("[pipeline] fatal error: {e}");
        return;
    }

    if tokio::signal::ctrl_c().await.is_ok() {
        println!("Shutting down…");
    }
    std::process::exit(0);
}