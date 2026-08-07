use std::sync::atomic::Ordering;
use tokio::sync::mpsc;

use crate::audio::{AudioEvent, AudioFormat, Speaker};
use crate::audio::hotkey::PauseFlag;
use crate::audio::normalizer::AudioNormalizer;
use crate::audio::vad::{SpeechTurn, VadChannel};
use crate::stt::SttSender;

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

pub async fn run_audio(
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


fn log_speech_turn(turn: &SpeechTurn) {
    println!("[TURN] {turn}");
}

fn insert_chronologically(conversation: &mut Vec<SpeechTurn>, turn: SpeechTurn) {
    let pos = conversation.partition_point(|t| t.start_ms <= turn.start_ms);
    conversation.insert(pos, turn);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::speaker_label;

    fn make_turn(start_ms: u128) -> SpeechTurn {
        SpeechTurn {
            speaker: Speaker::User,
            audio: vec![1, 2, 3],
            start_ms,
            end_ms: start_ms + 5,
        }
    }

    #[test]
    fn insert_chronologically_keeps_turns_sorted_by_start_time() {
        let mut conversation = vec![make_turn(20), make_turn(40)];

        insert_chronologically(&mut conversation, make_turn(30));

        let starts: Vec<u128> = conversation.iter().map(|turn| turn.start_ms).collect();
        assert_eq!(starts, vec![20, 30, 40]);
    }

    #[test]
    fn speaker_label_uses_expected_labels() {
        assert_eq!(speaker_label(&Speaker::User), "[User]");
        assert_eq!(speaker_label(&Speaker::System), "[Interviewer]");
    }
}