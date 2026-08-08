mod overlay;

use tokio::sync::oneshot;

use crate::audio::hotkey::PauseFlag;
use crate::transcript::LiveTranscript;

pub fn run_blocking(
    pause_flag:      PauseFlag,
    live_transcript: LiveTranscript,
    context_tx:      oneshot::Sender<String>,
) {
    overlay::run_window(pause_flag, live_transcript, context_tx);
}