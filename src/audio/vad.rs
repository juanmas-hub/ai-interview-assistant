use std::fmt;
use std::sync::{Arc, LazyLock, Mutex};
use anyhow::Result;
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

use crate::config;
use super::Speaker;

const MODEL_BYTES: &[u8] = include_bytes!("silero_vad.onnx");

static SHARED_SESSION: LazyLock<Arc<Mutex<Session>>> = LazyLock::new(|| {
    Arc::new(Mutex::new(
        Session::builder()
            .unwrap()
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .unwrap()
            .with_intra_threads(1)
            .unwrap()
            .with_inter_threads(1)
            .unwrap()
            .commit_from_memory(MODEL_BYTES)
            .unwrap(),
    ))
});

const SILERO_STATE_SIZE: usize = 2 * 1 * 128;

struct SileroVad {
    session: Arc<Mutex<Session>>,
    state:   Vec<f32>,
}

impl SileroVad {
    fn new() -> Self {
        Self {
            session: SHARED_SESSION.clone(),
            state:   vec![0.0f32; SILERO_STATE_SIZE],
        }
    }

    fn speech_probability(&mut self, chunk: &[f32]) -> f32 {
        assert_eq!(
            chunk.len(),
            config::vad::CHUNK_SAMPLES,
            "Silero VAD requires exactly {} samples",
            config::vad::CHUNK_SAMPLES,
        );

        let audio = Tensor::from_array((
            [1usize, config::vad::CHUNK_SAMPLES],
            chunk.to_vec().into_boxed_slice(),
        )).unwrap();

        let state = Tensor::from_array((
            [2usize, 1usize, 128usize],
            self.state.clone().into_boxed_slice(),
        )).unwrap();

        let sr = Tensor::from_array((
            [1usize],
            vec![16_000i64].into_boxed_slice(),
        )).unwrap();

        let mut session = self.session.lock().unwrap();
        let outputs = session
            .run(ort::inputs!["input" => audio, "state" => state, "sr" => sr])
            .unwrap();

        let (_, new_state) = outputs["stateN"].try_extract_tensor::<f32>().unwrap();
        self.state.clear();
        self.state.extend_from_slice(new_state);

        outputs["output"].try_extract_tensor::<f32>().unwrap().1[0]
    }

    fn reset_state(&mut self) {
        self.state.fill(0.0);
    }
}

#[derive(Debug, Clone)]
pub struct SpeechTurn {
    pub speaker:  Speaker,
    pub audio:    Vec<i16>,
    pub start_ms: u128,
    pub end_ms:   u128,
}

impl SpeechTurn {
    pub fn duration_secs(&self) -> f32 {
        self.audio.len() as f32 / config::resampler::TARGET_SAMPLE_RATE as f32
    }
}

impl fmt::Display for SpeechTurn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} | {:.2}s | {} samples | {}ms–{}ms",
            self.speaker,
            self.duration_secs(),
            self.audio.len(),
            self.start_ms,
            self.end_ms,
        )
    }
}

enum TurnState {
    Silence,
    Speech {
        chunk_count:   usize,
        audio:         Vec<i16>,
        start_ms:      u128,
        hangover_left: usize,
    },
}

pub struct VadChannel {
    vad:        SileroVad,
    speaker:    Speaker,
    sample_buf: Vec<i16>,
    state:      TurnState,
}

impl VadChannel {
    pub fn new(speaker: Speaker) -> Result<Self> {
        println!(
            "[VAD] Silero ready for {speaker} — chunk_samples={} threshold={}",
            config::vad::CHUNK_SAMPLES,
            config::vad::SPEECH_THRESHOLD,
        );
        Ok(Self {
            vad:        SileroVad::new(),
            speaker,
            sample_buf: Vec::new(),
            state:      TurnState::Silence,
        })
    }

    pub fn push(&mut self, samples: &[i16]) -> Vec<SpeechTurn> {
        self.sample_buf.extend_from_slice(samples);
        let mut completed = Vec::new();

        while self.sample_buf.len() >= config::vad::CHUNK_SAMPLES {
            let chunk: Vec<i16> = self.sample_buf.drain(..config::vad::CHUNK_SAMPLES).collect();
            let f32_chunk: Vec<f32> = chunk.iter().map(|&s| s as f32 / 32_768.0).collect();

            let is_speech = self.vad.speech_probability(&f32_chunk) >= config::vad::SPEECH_THRESHOLD;
            self.advance_state(is_speech, &chunk, &mut completed);
        }

        completed
    }

    fn advance_state(&mut self, is_speech: bool, chunk: &[i16], completed: &mut Vec<SpeechTurn>) {
        self.state = match (std::mem::replace(&mut self.state, TurnState::Silence), is_speech) {

            (TurnState::Silence, true) => {
                println!("[VAD] {speaker} ▶ speech started", speaker = self.speaker);
                TurnState::Speech {
                    chunk_count:   1,
                    audio:         chunk.to_vec(),
                    start_ms:      now_ms(),
                    hangover_left: config::vad::HANGOVER_CHUNKS,
                }
            }

            (TurnState::Speech { chunk_count, mut audio, start_ms, .. }, true) => {
                audio.extend_from_slice(chunk);
                TurnState::Speech {
                    chunk_count:   chunk_count + 1,
                    audio,
                    start_ms,
                    hangover_left: config::vad::HANGOVER_CHUNKS,
                }
            }

            (TurnState::Speech { chunk_count, mut audio, start_ms, hangover_left }, false)
                if hangover_left > 1 =>
            {
                audio.extend_from_slice(chunk);
                TurnState::Speech {
                    chunk_count,
                    audio,
                    start_ms,
                    hangover_left: hangover_left - 1,
                }
            }

            (TurnState::Speech { chunk_count, mut audio, start_ms, .. }, false) => {
                audio.extend_from_slice(chunk);
                let end_ms = now_ms();

                if chunk_count >= config::vad::MIN_SPEECH_CHUNKS {
                    println!(
                        "[VAD] {speaker} ■ turn ended ({chunk_count} chunks, {}ms)",
                        end_ms.saturating_sub(start_ms),
                        speaker = self.speaker,
                    );
                    completed.push(SpeechTurn {
                        speaker: self.speaker,
                        audio,
                        start_ms,
                        end_ms,
                    });
                } else {
                    println!(
                        "[VAD] {speaker} ✗ noise burst discarded ({chunk_count} chunks)",
                        speaker = self.speaker,
                    );
                }

                self.vad.reset_state();
                TurnState::Silence
            }

            (TurnState::Silence, false) => TurnState::Silence,
        };
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}