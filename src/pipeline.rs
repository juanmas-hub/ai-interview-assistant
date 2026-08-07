use anyhow::Result;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::ai::RagEngine;
use crate::ai::dispatch::run_ai;
use crate::audio::AudioEvent;
use crate::audio::Speaker;
use crate::audio::router::run_audio;
use crate::config::{self, Environment};
use crate::stt::{SttSender, TurnComplete};
use crate::stt::deepgram::DeepgramSender;
use crate::transcript::Transcript;

pub async fn start(env: Environment) -> Result<()> {
    let context    = crate::ui::prompt_user_context();
    let rag_engine = Arc::new(RagEngine::load(&context).await?);
    let transcript = Arc::new(Mutex::new(Transcript::open(config::transcript::PATH)));

    let (audio_tx,         audio_rx)        = mpsc::channel::<AudioEvent>(1_000);
    let (turn_complete_tx, turn_complete_rx) = mpsc::channel::<TurnComplete>(256);
    let (ai_tx,            ai_rx)            = mpsc::channel::<TurnComplete>(256);

    let user_stt   = connect_stt(Speaker::User,   turn_complete_tx.clone(), &env).await?;
    let system_stt = connect_stt(Speaker::System, turn_complete_tx,         &env).await?;

    crate::audio::wasapi::start_concurrent_capture(audio_tx)?;

    tokio::spawn(run_audio(audio_rx, env.pause_flag, user_stt, system_stt));
    tokio::spawn(crate::stt::run(turn_complete_rx, ai_tx, transcript.clone()));
    tokio::spawn(run_ai(ai_rx, rag_engine, transcript));

    Ok(())
}

async fn connect_stt(
    speaker: Speaker,
    tx:      mpsc::Sender<TurnComplete>,
    env:     &Environment,
) -> Result<Box<dyn SttSender>> {
    Ok(Box::new(
        DeepgramSender::connect(speaker, tx, &env.deepgram_api_key).await?
    ))
}