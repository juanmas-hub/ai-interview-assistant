use tokio::sync::mpsc;
use std::sync::atomic::Ordering;

use crate::audio::{AudioEvent, AudioFormat, Speaker};
use crate::audio::hotkey::PauseFlag;
use crate::audio::normalizer::AudioNormalizer;
use crate::audio::vad::{SpeechTurn, VadChannel};
//use crate::audio::wav_writer::{CaptureRecorder, save_turn_as_wav};
use crate::stt::SttSender;

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

        self.stt.send_audio(&normalized);  // fluye a Deepgram continuamente
        self.vad.push(&normalized)         // VAD detecta cuándo termina el turno
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

    //let mut recorder    = CaptureRecorder::new();
    let mut conversation: Vec<SpeechTurn> = Vec::new();

    while let Some(event) = rx.recv().await {
        if pause_flag.load(Ordering::Relaxed) { continue; }

        match event {
            AudioEvent::RawCapture { speaker, samples, format } => {
                //recorder.record_chunk(speaker, &samples, format);

                let processor = match speaker {
                    Speaker::User   => &mut user,
                    Speaker::System => &mut system,
                };

                for turn in processor.process(&samples, format) {
                    processor.stt.end_turn();  // VAD terminó — recogé el texto acumulado
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

fn on_turn_completed(turn: &SpeechTurn) {
    println!("[TURN] {turn}");
    /*if let Err(e) = save_turn_as_wav(turn.speaker, turn.start_ms, &turn.audio) {
        eprintln!("[wav] Failed to save turn: {e}");
    }*/
}

fn insert_chronologically(conversation: &mut Vec<SpeechTurn>, turn: SpeechTurn) {
    let pos = conversation.partition_point(|t| t.start_ms <= turn.start_ms);
    conversation.insert(pos, turn);
}