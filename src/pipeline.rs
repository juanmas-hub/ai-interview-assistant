use tokio::sync::mpsc;
use anyhow::Result;
use std::sync::atomic::Ordering;
use crate::config::Environment;
use crate::stt::deepgram::DeepgramSender;
use crate::audio::{AudioEvent, AudioFormat, Speaker};
use crate::audio::hotkey::PauseFlag;
use crate::audio::normalizer::AudioNormalizer;
use crate::audio::vad::{SpeechTurn, VadChannel};
use crate::audio::wasapi::start_concurrent_capture;
use crate::stt::{SttSender, TurnComplete};
use crate::stt;

pub async fn start(env: Environment) -> Result<()> {
    let (audio_tx,         audio_rx)        = mpsc::channel::<AudioEvent>(1_000);
    let (turn_complete_tx, turn_complete_rx) = mpsc::channel::<TurnComplete>(256);
    let (ai_tx,            ai_rx)            = mpsc::channel::<TurnComplete>(256);

    let user_stt:   Box<dyn SttSender> = Box::new(
        DeepgramSender::connect(Speaker::User,   turn_complete_tx.clone(), &env.deepgram_api_key).await?
    );
    let system_stt: Box<dyn SttSender> = Box::new(
        DeepgramSender::connect(Speaker::System, turn_complete_tx,         &env.deepgram_api_key).await?
    );

    start_concurrent_capture(audio_tx)?;

    tokio::spawn(run(audio_rx, env.pause_flag, user_stt, system_stt));
    tokio::spawn(run_ai(ai_rx));
    tokio::spawn(stt::run(turn_complete_rx, ai_tx));

    Ok(())
}

pub struct AudioProcessor {
    normalizer: AudioNormalizer,
    vad:        VadChannel,
    stt:        Box<dyn SttSender>,
}

impl AudioProcessor {
    pub fn new(speaker: Speaker, stt: Box<dyn SttSender>) -> Self {
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
        self.vad.push(&normalized)
    }
}

pub async fn run(
    mut rx:     mpsc::Receiver<AudioEvent>,
    pause_flag: PauseFlag,
    user_stt:   Box<dyn SttSender>,
    system_stt: Box<dyn SttSender>,
) {
    let mut user   = AudioProcessor::new(Speaker::User,   user_stt);
    let mut system = AudioProcessor::new(Speaker::System, system_stt);
    let mut conversation: Vec<SpeechTurn> = Vec::new();

    while let Some(event) = rx.recv().await {
        if pause_flag.load(Ordering::Relaxed) { continue; }

        match event {
            AudioEvent::RawCapture { speaker, samples, format } => {
                let processor = match speaker {
                    Speaker::User   => &mut user,
                    Speaker::System => &mut system,
                };

                for turn in processor.process(&samples, format) {
                    processor.stt.end_turn();
                    on_turn_completed(&turn);
                    insert_chronologically(&mut conversation, turn);
                }
            }
            AudioEvent::CaptureError { speaker, error } => {
                eprintln!("[audio] {speaker} capture error: {error}");
            }
        }
    }
}

pub async fn run_ai(mut rx: mpsc::Receiver<TurnComplete>) {
    while let Some(turn) = rx.recv().await {
        if turn.speaker != Speaker::System { continue; }

        tokio::spawn(async move {
            match crate::ai::run(&turn.text).await {
                Ok(response) => println!("[AI] {response}"),
                Err(e)       => eprintln!("[ai] error: {e}"),
            }
        });
    }
}

fn on_turn_completed(turn: &SpeechTurn) {
    println!("[TURN] {turn}");
}

fn insert_chronologically(conversation: &mut Vec<SpeechTurn>, turn: SpeechTurn) {
    let pos = conversation.partition_point(|t| t.start_ms <= turn.start_ms);
    conversation.insert(pos, turn);
}