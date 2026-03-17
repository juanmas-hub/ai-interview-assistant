use crate::audio::hotkey::{self, PauseFlag};

pub mod capture {
    pub const RING_BUFFER_CAPACITY: usize = 48_000 * 2 * 4;
    pub const CONSUMER_CHUNK_SIZE:  usize = 4_096;
    pub const EVENT_TIMEOUT_MS:     u32   = 6 * 60 * 60 * 1_000;
}

pub mod resampler {
    pub const TARGET_SAMPLE_RATE:  u32   = 16_000;
    pub const INPUT_CHUNK_FRAMES:  usize = 1_024;
    pub const SUB_CHUNKS:          usize = 2;
}

pub mod vad {
    pub const CHUNK_SAMPLES:     usize = 512;
    pub const SPEECH_THRESHOLD:  f32   = 0.50;
    pub const HANGOVER_CHUNKS:   usize = 20;
    pub const MIN_SPEECH_CHUNKS: usize = 3;
}

pub mod deepgram {
    pub const WS_URL: &str = concat!(
        "wss://api.deepgram.com/v1/listen",
        "?model=nova-2",
        "&language=es",
        "&encoding=linear16",
        "&sample_rate=16000",
        "&channels=1",
        "&interim_results=true",
        "&endpointing=300",
    );
}

pub struct Environment {
    pub deepgram_api_key: String,
    pub pause_flag:       PauseFlag,
}

impl Environment {
    pub fn load() -> Self {
        dotenvy::dotenv().ok();

        let deepgram_api_key = std::env::var("DEEPGRAM_API_KEY")
            .expect("DEEPGRAM_API_KEY env var not set");

        let pause_flag = hotkey::new_pause_flag();
        hotkey::spawn_hotkey_listener(pause_flag.clone());

        Self { deepgram_api_key, pause_flag }
    }
}