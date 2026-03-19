use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::io::Write;
use tokio::sync::mpsc;

use crate::audio::{AudioEvent, AudioFormat, Speaker};
use crate::audio::hotkey::PauseFlag;
use crate::audio::normalizer::AudioNormalizer;
use crate::audio::vad::{SpeechTurn, VadChannel};
use crate::stt::{SttSender, TurnComplete};
use crate::stt::deepgram::DeepgramSender;
use crate::ai::{AiServices, vector_store::VectorStore};
use crate::config::Environment;

struct Pipeline {
    audio_rx:         mpsc::Receiver<AudioEvent>,
    pause_flag:       PauseFlag,
    user_stt:         Box<dyn SttSender>,
    system_stt:       Box<dyn SttSender>,
    turn_complete_rx: mpsc::Receiver<TurnComplete>,
    ai_tx:            mpsc::Sender<TurnComplete>,
    ai_rx:            mpsc::Receiver<TurnComplete>,
    store:            Arc<VectorStore>,
    services:         Arc<AiServices>,
}

struct AudioProcessor {
    normalizer: AudioNormalizer,
    vad:        VadChannel,
    stt:        Box<dyn SttSender>,
}

struct AudioRouter {
    user:         AudioProcessor,
    system:       AudioProcessor,
    conversation: Vec<SpeechTurn>,
}

pub async fn start(env: Environment) -> Result<()> {
    let pipeline = build(env).await?;
    spawn(pipeline);
    Ok(())
}

async fn build(env: Environment) -> Result<Pipeline> {
    let services = Arc::new(AiServices::load()?);
    let context  = crate::ui::prompt_user_context();
    let store    = Arc::new(crate::setup::load(&context, &services).await?);

    let (audio_tx,         audio_rx)         = mpsc::channel::<AudioEvent>(1_000);
    let (turn_complete_tx, turn_complete_rx)  = mpsc::channel::<TurnComplete>(256);
    let (ai_tx,            ai_rx)             = mpsc::channel::<TurnComplete>(256);

    let user_stt   = connect_stt(Speaker::User,   turn_complete_tx.clone(), &env).await?;
    let system_stt = connect_stt(Speaker::System, turn_complete_tx,         &env).await?;

    crate::audio::wasapi::start_concurrent_capture(audio_tx)?;

    Ok(Pipeline { audio_rx, pause_flag: env.pause_flag, user_stt, system_stt,
                  turn_complete_rx, ai_tx, ai_rx, store, services })
}

fn spawn(p: Pipeline) {
    tokio::spawn(run_audio(p.audio_rx, p.pause_flag, p.user_stt, p.system_stt));
    tokio::spawn(crate::stt::run(p.turn_complete_rx, p.ai_tx));
    tokio::spawn(run_ai(p.ai_rx, p.store, p.services));
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

impl AudioProcessor {
    fn new(speaker: Speaker, stt: Box<dyn SttSender>) -> Self {
        Self {
            normalizer: AudioNormalizer::new(),
            vad:        VadChannel::new(speaker).expect("failed to initialise VAD channel"),
            stt,
        }
    }

    fn process(&mut self, samples: &[f32], format: AudioFormat) -> Vec<SpeechTurn> {
        let normalized = match self.normalizer.process(samples, format) {
            Ok(n) if !n.is_empty() => n,
            Ok(_)  => return vec![],
            Err(e) => { eprintln!("[normalizer] error: {e}"); return vec![]; }
        };

        self.stt.send_audio(&normalized);
        let turns = self.vad.push(&normalized);

        for _ in &turns {
            self.stt.end_turn();
        }

        turns
    }
}

impl AudioRouter {
    fn new(user_stt: Box<dyn SttSender>, system_stt: Box<dyn SttSender>) -> Self {
        Self {
            user:         AudioProcessor::new(Speaker::User,   user_stt),
            system:       AudioProcessor::new(Speaker::System, system_stt),
            conversation: Vec::new(),
        }
    }

    fn handle(&mut self, event: AudioEvent) {
        match event {
            AudioEvent::RawCapture { speaker, samples, format } => {
                self.on_capture(speaker, &samples, format);
            }
            AudioEvent::CaptureError { speaker, error } => {
                eprintln!("[audio] {speaker} capture error: {error}");
            }
        }
    }

    fn on_capture(&mut self, speaker: Speaker, samples: &[f32], format: AudioFormat) {
        for turn in self.processor(speaker).process(samples, format) {
            log_speech_turn(&turn);
            insert_chronologically(&mut self.conversation, turn);
        }
    }

    fn processor(&mut self, speaker: Speaker) -> &mut AudioProcessor {
        match speaker {
            Speaker::User   => &mut self.user,
            Speaker::System => &mut self.system,
        }
    }
}

async fn run_audio(
    mut rx:     mpsc::Receiver<AudioEvent>,
    pause_flag: PauseFlag,
    user_stt:   Box<dyn SttSender>,
    system_stt: Box<dyn SttSender>,
) {
    let mut router = AudioRouter::new(user_stt, system_stt);

    while let Some(event) = rx.recv().await {
        if pause_flag.load(Ordering::Relaxed) { continue; }
        router.handle(event);
    }
}

async fn run_ai(
    mut rx:   mpsc::Receiver<TurnComplete>,
    store:    Arc<VectorStore>,
    services: Arc<AiServices>,
) {
    while let Some(turn) = rx.recv().await {
        if !is_interviewer_question(&turn) { continue; }
        answer_in_background(turn, store.clone(), services.clone());
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn is_interviewer_question(turn: &TurnComplete) -> bool {
    turn.speaker == Speaker::System
}

fn answer_in_background(turn: TurnComplete, store: Arc<VectorStore>, services: Arc<AiServices>) {
    tokio::spawn(async move {
        match crate::ai::answer(&turn.text, &store, &services).await {
            Ok(response) => output_ai_response(&response),
            Err(e)       => eprintln!("[ai] error: {e}"),
        }
    });
}

fn output_ai_response(response: &str) {
    print_response(response);
    append_to_transcript(response);
}

fn print_response(response: &str) {
    println!("[AI] {response}");
}

fn append_to_transcript(response: &str) {
    let line   = format!("[AI]: {response}\n");
    let result = std::fs::OpenOptions::new()
        .append(true)
        .open("transcript.txt")
        .and_then(|mut f| f.write_all(line.as_bytes()));

    if let Err(e) = result {
        eprintln!("[ai] error writing to transcript: {e}");
    }
}

fn log_speech_turn(turn: &SpeechTurn) {
    println!("[TURN] {turn}");
}

fn insert_chronologically(conversation: &mut Vec<SpeechTurn>, turn: SpeechTurn) {
    let pos = conversation.partition_point(|t| t.start_ms <= turn.start_ms);
    conversation.insert(pos, turn);
}