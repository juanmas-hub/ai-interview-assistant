use std::fmt;
use std::sync::{Arc, LazyLock, Mutex};
use anyhow::Result;
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};

use crate::config;
use super::Speaker;

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Abstracción sobre cualquier detector de actividad de voz.
/// Separa la inferencia ML de la máquina de estados del turno.
pub trait VoiceDetector: Send {
    /// Devuelve true si el chunk de audio contiene voz.
    fn is_speech(&mut self, chunk: &[f32]) -> bool;
    /// Reinicia el estado interno del modelo entre turnos.
    fn reset(&mut self);
}

// ── SileroVad ─────────────────────────────────────────────────────────────────

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
}

impl VoiceDetector for SileroVad {
    fn is_speech(&mut self, chunk: &[f32]) -> bool {
        self.speech_probability(chunk) >= config::vad::SPEECH_THRESHOLD
    }

    fn reset(&mut self) {
        self.state.fill(0.0);
    }
}

// ── SpeechTurn ────────────────────────────────────────────────────────────────

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

// ── TurnState ─────────────────────────────────────────────────────────────────

enum TurnState {
    Silence,
    Speech {
        chunk_count:   usize,
        audio:         Vec<i16>,
        start_ms:      u128,
        hangover_left: usize,
    },
}

// ── VadChannel ────────────────────────────────────────────────────────────────

/// Máquina de estados que detecta y delimita turnos de voz.
/// Delega la inferencia acústica al VoiceDetector inyectado.
pub struct VadChannel {
    vad:        Box<dyn VoiceDetector>,
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
            vad:        Box::new(SileroVad::new()),
            speaker,
            sample_buf: Vec::new(),
            state:      TurnState::Silence,
        })
    }

    /// Ingresa muestras, detecta voz chunk a chunk y devuelve los turnos completados.
    pub fn push(&mut self, samples: &[i16]) -> Vec<SpeechTurn> {
        self.sample_buf.extend_from_slice(samples);
        let mut completed = Vec::new();

        while self.sample_buf.len() >= config::vad::CHUNK_SAMPLES {
            let chunk = self.drain_next_chunk();
            let f32_chunk = i16_chunk_to_f32(&chunk);
            let is_speech = self.vad.is_speech(&f32_chunk);
            self.advance_state(is_speech, &chunk, &mut completed);
        }

        completed
    }

    // ── Máquina de estados ────────────────────────────────────────────────────

    fn advance_state(&mut self, is_speech: bool, chunk: &[i16], completed: &mut Vec<SpeechTurn>) {
        // Reemplazamos self.state antes de llamar métodos que necesitan &mut self.
        let old_state = std::mem::replace(&mut self.state, TurnState::Silence);
        self.state = self.transition(old_state, is_speech, chunk, completed);
    }

    fn transition(
        &mut self,
        state:     TurnState,
        is_speech: bool,
        chunk:     &[i16],
        completed: &mut Vec<SpeechTurn>,
    ) -> TurnState {
        match (state, is_speech) {
            (TurnState::Silence, true) =>
                self.on_speech_started(chunk),

            (TurnState::Silence, false) =>
                TurnState::Silence,

            (TurnState::Speech { chunk_count, audio, start_ms, .. }, true) =>
                speech_continued(chunk_count, audio, start_ms, chunk),

            (TurnState::Speech { chunk_count, audio, start_ms, hangover_left }, false)
                if hangover_left > 1 =>
                speech_in_hangover(chunk_count, audio, start_ms, hangover_left, chunk),

            (TurnState::Speech { chunk_count, audio, start_ms, .. }, false) =>
                self.on_speech_ended(chunk_count, audio, start_ms, chunk, completed),
        }
    }

    // ── Transiciones ──────────────────────────────────────────────────────────

    fn on_speech_started(&self, chunk: &[i16]) -> TurnState {
        println!("[VAD] {} ▶ speech started", self.speaker);
        TurnState::Speech {
            chunk_count:   1,
            audio:         chunk.to_vec(),
            start_ms:      now_ms(),
            hangover_left: config::vad::HANGOVER_CHUNKS,
        }
    }

    fn on_speech_ended(
        &mut self,
        chunk_count: usize,
        mut audio:   Vec<i16>,
        start_ms:    u128,
        chunk:       &[i16],
        completed:   &mut Vec<SpeechTurn>,
    ) -> TurnState {
        audio.extend_from_slice(chunk);
        let end_ms = now_ms();

        if chunk_count >= config::vad::MIN_SPEECH_CHUNKS {
            self.emit_turn(audio, start_ms, end_ms, chunk_count, completed);
        } else {
            println!("[VAD] {} ✗ noise burst discarded ({chunk_count} chunks)", self.speaker);
        }

        self.vad.reset();
        TurnState::Silence
    }

    fn emit_turn(
        &self,
        audio:       Vec<i16>,
        start_ms:    u128,
        end_ms:      u128,
        chunk_count: usize,
        completed:   &mut Vec<SpeechTurn>,
    ) {
        println!(
            "[VAD] {} ■ turn ended ({chunk_count} chunks, {}ms)",
            self.speaker,
            end_ms.saturating_sub(start_ms),
        );
        completed.push(SpeechTurn { speaker: self.speaker, audio, start_ms, end_ms });
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn drain_next_chunk(&mut self) -> Vec<i16> {
        self.sample_buf.drain(..config::vad::CHUNK_SAMPLES).collect()
    }
}

// ── Transiciones puras (sin acceso a self) ────────────────────────────────────

fn speech_continued(chunk_count: usize, mut audio: Vec<i16>, start_ms: u128, chunk: &[i16]) -> TurnState {
    audio.extend_from_slice(chunk);
    TurnState::Speech {
        chunk_count:   chunk_count + 1,
        audio,
        start_ms,
        hangover_left: config::vad::HANGOVER_CHUNKS,
    }
}

fn speech_in_hangover(
    chunk_count:   usize,
    mut audio:     Vec<i16>,
    start_ms:      u128,
    hangover_left: usize,
    chunk:         &[i16],
) -> TurnState {
    audio.extend_from_slice(chunk);
    TurnState::Speech {
        chunk_count,
        audio,
        start_ms,
        hangover_left: hangover_left - 1,
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn i16_chunk_to_f32(chunk: &[i16]) -> Vec<f32> {
    chunk.iter().map(|&s| s as f32 / 32_768.0).collect()
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}