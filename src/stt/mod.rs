pub mod deepgram;

use crate::audio::{vad::SpeechTurn};


pub trait SttSender: Send + Sync + 'static {
    fn send_turn(&self, turn: &SpeechTurn);
}
