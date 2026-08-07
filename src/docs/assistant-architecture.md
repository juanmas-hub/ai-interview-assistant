# AI Interview Copilot: Architecture & System Documentation

## Table of Contents

- [AI Interview Copilot: Architecture \& System Documentation](#ai-interview-copilot-architecture--system-documentation)
  - [Table of Contents](#table-of-contents)
  - [1. System Overview](#1-system-overview)
  - [2. Tech Stack \& External Crates](#2-tech-stack--external-crates)
  - [3. Module Map](#3-module-map)
  - [4. Core Data Types](#4-core-data-types)
    - [`Speaker` (`audio/mod.rs`)](#speaker-audiomodrs)
    - [`AudioFormat` (`audio/mod.rs`)](#audioformat-audiomodrs)
    - [`AudioEvent` (`audio/mod.rs`)](#audioevent-audiomodrs)
    - [`TurnComplete` (`stt/mod.rs`)](#turncomplete-sttmodrs)
    - [`SpeechTurn` (`audio/vad.rs`)](#speechturn-audiovadrs)
    - [`SearchResult` (`ai/vector_store.rs`)](#searchresult-aivector_storers)
  - [5. Threads \& Async Tasks](#5-threads--async-tasks)
    - [Real OS threads (`std::thread::spawn`)](#real-os-threads-stdthreadspawn)
    - [Tokio tasks (`tokio::spawn`)](#tokio-tasks-tokiospawn)
    - [Why tokio at all](#why-tokio-at-all)
  - [6. End-to-End Data Flow](#6-end-to-end-data-flow)
  - [7. Component: Audio Capture (WASAPI)](#7-component-audio-capture-wasapi)
    - [`AudioSource`](#audiosource)
    - [Capture pipeline per source](#capture-pipeline-per-source)
    - [`SendableDevice`](#sendabledevice)
    - [Backpressure handling](#backpressure-handling)
  - [8. Component: Voice Activity Detection](#8-component-voice-activity-detection)
    - [`VoiceDetector` trait](#voicedetector-trait)
    - [`SileroVad`](#silerovad)
    - [`VadChannel` — the turn-boundary state machine](#vadchannel--the-turn-boundary-state-machine)
  - [9. Component: Speech-to-Text (Deepgram)](#9-component-speech-to-text-deepgram)
    - [Connection architecture](#connection-architecture)
    - [Reconnection](#reconnection)
    - [Transcript accumulation](#transcript-accumulation)
    - [Wire protocol](#wire-protocol)
  - [10. Component: RAG Engine](#10-component-rag-engine)
    - [`RagEngine::load(context: &str)`](#ragengineloadcontext-str)
    - [`RagEngine::answer(question: &str)`](#ragengineanswerquestion-str)
    - [`VectorStore` (`ai/vector_store.rs`)](#vectorstore-aivector_storers)
    - [Embedder / LLM clients](#embedder--llm-clients)
    - [`ai/dispatch.rs` — question filtering \& fan-out](#aidispatchrs--question-filtering--fan-out)
  - [11. Component: Transcript](#11-component-transcript)
  - [12. Component: Hotkey / Pause Control](#12-component-hotkey--pause-control)
  - [13. Design Patterns Used](#13-design-patterns-used)
  - [14. External Integrations \& Protocols](#14-external-integrations--protocols)
  - [15. Configuration](#15-configuration)
    - [`Environment` (also in `config.rs`)](#environment-also-in-configrs)
  - [16. Known Limitations \& Planned Work](#16-known-limitations--planned-work)

---

## 1. System Overview

AI Interview Copilot is a **real-time desktop assistant** (Windows-only) that listens to
both sides of an online technical interview (candidate's microphone + interviewer's
system audio via loopback), transcribes both streams concurrently, detects when the
interviewer finishes asking a question, and generates a concise, personalized answer
using retrieval-augmented generation (RAG) over the candidate's own background.

**Key properties:**

- **Concurrent dual-stream audio**: microphone and system loopback are captured,
  normalized, transcribed, and voice-activity-detected completely independently — the
  code path for one speaker never blocks the other.
- **Real-time, streaming**: every stage (capture → STT → RAG) is wired via async
  channels; there is no batch processing step.
- **Personalized**: the candidate's own background (typed once at startup) is embedded
  and retrieved semantically per question, rather than sent wholesale to the LLM.
- **Local-first VAD**: voice activity detection runs a local ONNX model (Silero VAD),
  not a cloud API — only finalized speech segments are sent onward.

**Trust model / data flow at a glance:**

```
Windows audio devices (mic + system loopback)
    ↕  WASAPI (native, blocking API)
OS threads → ring buffers → tokio channels
    ↕
Async pipeline (tokio tasks): normalize → VAD → STT (Deepgram WS) → RAG (Voyage + Groq)
    ↕
transcript.txt + stdout
```

The application has no server component and no persistence beyond a single append-only
transcript file — every run starts from a blank vector store built from whatever
context the user types in that session.

---

## 2. Tech Stack & External Crates

From `Cargo.toml`:

| Crate | Version | Purpose |
|---|---|---|
| `tokio` | 1.50 (`full`) | Async runtime — powers every task in the pipeline except raw audio capture |
| `anyhow` | 1.0 | Error propagation (`Result<T>` almost everywhere) |
| `wasapi` | 0.22 | Windows Audio Session API bindings — mic + system loopback capture |
| `windows-sys` | 0.59 | Low-level Win32 bindings — used specifically for the F9 hotkey (`GetAsyncKeyState`) |
| `ringbuf` | 0.4 | Lock-free SPSC ring buffer — bridges OS capture threads into the async world |
| `rubato` | 0.16 | Audio resampling (`FftFixedIn`) — 48kHz→16kHz conversion |
| `ort` | 2.0.0-rc.12 (`download-binaries`) | ONNX Runtime bindings — runs the embedded Silero VAD model |
| `tokio-tungstenite` | 0.21 (`native-tls`) | WebSocket client — Deepgram streaming STT connection |
| `futures-util` | 0.3 | Stream/sink combinators for the WebSocket split (`SplitSink`/`SplitStream`) |
| `serde` / `serde_json` | 1.x | (De)serialization — Deepgram responses, Voyage/Groq request/response bodies |
| `reqwest` | 0.12 (`json`) | HTTP client — Voyage AI embeddings, Groq chat completions |
| `async-trait` | 0.1 | Enables `async fn` in the `Embedder`/`Llm`/`SttSender` traits |
| `dotenvy` | 0.15 | Loads `.env` for API keys at startup |
| `hound` | 3.5 | WAV file I/O (used by `audio/wav_writer.rs`, not covered in detail here) |

**Not a workspace** — this is a single binary crate (`edition = "2024"`), unlike
multi-crate systems. Everything lives under `src/`.

---

## 3. Module Map

```
ai-interview-assistant (binary crate)
│
├── src/
│   ├── main.rs              # Entry point: loads Environment, starts hotkey listener,
│   │                         # calls pipeline::start(), waits on ctrl_c
│   │
│   ├── config.rs             # All tunable constants + Environment (env vars, PauseFlag)
│   │
│   ├── pipeline.rs           # Orchestrator: builds every dependency, wires channels,
│   │                         # spawns the 3 top-level tokio tasks
│   │
│   ├── transcript.rs         # Single owner of transcript.txt (shared via Arc<Mutex<_>>)
│   │
│   ├── audio/
│   │   ├── mod.rs            # Speaker, AudioFormat, AudioEvent — shared vocabulary
│   │   ├── router.rs         # AudioProcessor, AudioRouter, run_audio — per-speaker
│   │   │                     # normalize→VAD→STT routing
│   │   ├── normalizer.rs     # AudioNormalizer — resample + downmix + quantize to i16
│   │   ├── resampler.rs      # Resampler — wraps rubato::FftFixedIn
│   │   ├── vad.rs            # VoiceDetector trait, SileroVad, VadChannel (turn state machine)
│   │   ├── wasapi.rs         # OS-thread-based capture from Windows audio devices
│   │   ├── hotkey.rs         # F9 pause/resume listener (OS thread, polling GetAsyncKeyState)
│   │   ├── wav_writer.rs     # (not covered — WAV file debug output)
│   │   └── silero_vad.onnx   # Embedded VAD model (include_bytes!)
│   │
│   ├── stt/
│   │   ├── mod.rs            # TurnComplete, SttSender trait, run() — transcript logging + forward
│   │   └── deepgram.rs       # DeepgramSender — WebSocket client with auto-reconnect
│   │
│   └── ai/
│       ├── mod.rs            # RagEngine — owns embedder + llm + store, exposes answer()
│       ├── dispatch.rs       # run_ai — filters interviewer questions, dispatches to RagEngine
│       ├── embedder.rs       # Embedder trait, VoyageEmbedder (Voyage AI HTTP client)
│       ├── llm.rs            # Llm trait, GroqLlm (Groq HTTP client)
│       ├── prompt.rs         # Prompt construction (system + user message)
│       └── vector_store.rs   # VectorStore — in-memory cosine-similarity search
│
├── .env                       # DEEPGRAM_API_KEY, VOYAGE_API_KEY, GROQ_API_KEY
└── Cargo.toml
```

> **Note:** a `ui/` module is planned for a future visual overlay but does not exist
> yet — the current interaction model is entirely console-based (`stdout` +
> `transcript.txt`). Not covered in this document.

---

## 4. Core Data Types

These types form the shared vocabulary that flows across module boundaries.

### `Speaker` (`audio/mod.rs`)

```rust
pub enum Speaker { User, System }
```

`User` = microphone (the candidate). `System` = loopback output (the interviewer, in a
typical call setup). This single enum is what lets one code path serve both audio
streams without duplicating logic — it's threaded through nearly every struct and
message type in the system (`AudioEvent`, `TurnComplete`, `SpeechTurn`).

### `AudioFormat` (`audio/mod.rs`)

```rust
pub struct AudioFormat { pub sample_rate: u32, pub channels: u16 }
```

Minimal `Copy` struct describing the format WASAPI actually delivered — used by the
normalizer to know how to resample/downmix.

### `AudioEvent` (`audio/mod.rs`)

```rust
pub enum AudioEvent {
    RawCapture { speaker: Speaker, samples: Vec<f32>, format: AudioFormat },
    CaptureError { speaker: Speaker, error: String },
}
```

The message type crossing the boundary from OS capture threads into the async
pipeline. `samples: Vec<f32>` are raw PCM samples in `-1.0..=1.0`.

### `TurnComplete` (`stt/mod.rs`)

```rust
pub struct TurnComplete { pub speaker: Speaker, pub text: String }
```

Emitted once Deepgram finalizes a transcript for a given speech turn (i.e. VAD detected
silence and `end_turn()` was called).

### `SpeechTurn` (`audio/vad.rs`)

```rust
pub struct SpeechTurn {
    pub speaker: Speaker,
    pub audio: Vec<i16>,
    pub start_ms: u128,
    pub end_ms: u128,
}
```

A delimited segment of speech produced by the VAD state machine. Note: this carries
**audio samples and timing**, not text — text arrives separately and later, from
Deepgram.

### `SearchResult` (`ai/vector_store.rs`)

```rust
pub struct SearchResult { pub payload: String, pub score: f32 }
```

A scored match from the vector store — `payload` is the original text chunk of the
candidate's background, `score` is cosine similarity to the query.

---

## 5. Threads & Async Tasks

This is one of the more important sections to get right — the system mixes **real OS
threads** (`std::thread`) with **tokio async tasks**, and confusing the two leads to
wrong assumptions about scheduling and blocking.

### Real OS threads (`std::thread::spawn`)

| Thread | Spawned in | Purpose |
|---|---|---|
| Fill thread (mic) | `wasapi::spawn_fill_thread` via `start_concurrent_capture` | Blocks on WASAPI's event handle, drains the device buffer into a ring buffer |
| Forward thread (mic) | `wasapi::spawn_forward_thread` | Drains the ring buffer, sends `AudioEvent` via `blocking_send` |
| Fill thread (system loopback) | same, second call with `AudioSource::SystemLoopback` | Same role, other device |
| Forward thread (system loopback) | same | Same role, other device |
| Hotkey listener | `hotkey::spawn_hotkey_listener` (called from `main.rs` via `Environment::start_hotkey_listener`) | Polls `GetAsyncKeyState(VK_F9)` every 30ms, toggles `PauseFlag` |

**Total: 5 dedicated OS threads**, all blocking/polling loops that never touch the
tokio runtime directly. They exist because WASAPI and `GetAsyncKeyState` are
synchronous Win32 APIs with no async equivalent — bridging them into tokio via
`spawn_blocking` was an option not taken; instead they communicate outward via a
lock-free ring buffer (`ringbuf`) and a shared `AtomicBool` (`PauseFlag`), respectively.

Why **two** threads per audio source instead of one: `fill_ring_buffer` blocks on
`event_handle.wait_for_event(...)` (a WASAPI-level wait), while `forward_raw_audio`
polls the ring buffer with a 1ms sleep when empty. Splitting them means a slow consumer
(forwarding into a possibly-full tokio channel via `blocking_send`) can never delay the
producer draining the device buffer — the ring buffer absorbs the difference, and
overflow is logged (`push_to_ring_buffer`) rather than silently dropped without trace.

### Tokio tasks (`tokio::spawn`)

| Task | Spawned in | Lifetime |
|---|---|---|
| `run_audio` | `pipeline::start` | Application lifetime |
| `stt::run` | `pipeline::start` | Application lifetime |
| `run_ai` (`ai::dispatch`) | `pipeline::start` | Application lifetime |
| `DeepgramConnection::supervise` (×2, one per `Speaker`) | `DeepgramSender::connect` | Application lifetime — internally loops forever, reconnecting on stream close |
| `sender_task` | `DeepgramConnection::open_session`, inside `supervise`'s loop | One per WebSocket session — re-spawned on every reconnect |
| `answer_in_background` | `ai::dispatch::run_ai`, per qualifying question | Ephemeral — one per interviewer question that gets answered |

The three top-level tasks form the actual pipeline; everything else is either a fixed
support task (Deepgram connection management) or dynamically spawned per unit of work
(one task per AI answer, so a slow Groq response never blocks the next question from
being detected).

### Why tokio at all

Every I/O-bound external call in this system is naturally async: two persistent
Deepgram WebSocket connections, HTTP calls to Voyage AI and Groq. tokio lets these run
concurrently as lightweight tasks multiplexed over a thread pool, rather than
dedicating an OS thread to each blocked connection.

---

## 6. End-to-End Data Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│ OS THREADS (audio/wasapi.rs)                                        │
│                                                                       │
│  Mic device ──fill──▶ ring buffer ──forward──▶ AudioEvent::RawCapture│
│  Loopback   ──fill──▶ ring buffer ──forward──▶ AudioEvent::RawCapture│
└───────────────────────────────┬───────────────────────────────────────┘
                                 │ audio_tx (mpsc, cap 1000)
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│ TOKIO TASK: run_audio (audio/router.rs)                             │
│                                                                       │
│  AudioRouter routes by Speaker to the matching AudioProcessor:      │
│    1. AudioNormalizer.process() → resample 48kHz→16kHz, downmix,    │
│       quantize f32→i16                                              │
│    2. stt.send_audio(i16 samples)  ───────────────────┐             │
│    3. VadChannel.push() → detects turn boundaries      │             │
│    4. on turn boundary: stt.end_turn()  ───────────────┤             │
└───────────────────────────────┬─────────────────────────┼─────────────┘
                                 │                          │
                                 │ (samples)                │ (end_turn signal)
                                 ▼                          ▼
┌─────────────────────────────────────────────────────────────────────┐
│ DeepgramConnection (stt/deepgram.rs) — one per Speaker               │
│                                                                       │
│  Streams i16 PCM over WebSocket to Deepgram (linear16, 16kHz).      │
│  Accumulates "is_final" transcript fragments.                       │
│  On end_turn signal: flushes accumulated text as TurnComplete.      │
└───────────────────────────────┬───────────────────────────────────────┘
                                 │ turn_complete_tx (mpsc, cap 256)
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│ TOKIO TASK: stt::run (stt/mod.rs)                                    │
│                                                                       │
│  Skips empty turns. Logs every turn to Transcript                   │
│  ("[User]: ..." / "[Interviewer]: ..."). Forwards to ai_tx.         │
└───────────────────────────────┬───────────────────────────────────────┘
                                 │ ai_tx (mpsc, cap 256)
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│ TOKIO TASK: run_ai (ai/dispatch.rs)                                  │
│                                                                       │
│  Filters: only Speaker::System (interviewer) turns proceed.          │
│  Spawns an ephemeral task per question → RagEngine.answer():        │
│    1. Embed the question (Voyage AI)                                │
│    2. VectorStore.search() → top-K chunks above MIN_SCORE            │
│    3. prompt::build() → system + user message                       │
│    4. GroqLlm.complete() → bullet-point answer                      │
│  Result logged to Transcript ("[AI] ...") and stdout.                │
└─────────────────────────────────────────────────────────────────────┘
```

**Setup-time flow (runs once, before any of the above starts):**

```
ui::prompt_user_context() → user types background, one idea per line
    ↓
RagEngine::load(context)
    1. Creates VoyageEmbedder + GroqLlm clients
    2. chunk_context() → splits into lines
    3. embed_and_build_store() → Voyage embeds all lines, VectorStore.upsert() each
    ↓
Arc<RagEngine> ready — shared into run_ai
```

---

## 7. Component: Audio Capture (WASAPI)

`audio/wasapi.rs` is the only module in the system that talks directly to Windows.

### `AudioSource`

```rust
enum AudioSource { Microphone, SystemLoopback }
```

Maps to a WASAPI `Direction` (`Capture` for mic, `Render` for loopback — capturing from
a render endpoint is how Windows exposes "what's playing through the speakers") and to
a `Speaker` value.

### Capture pipeline per source

`spawn_capture_pipeline` does, for each source:
1. `open_source` — initializes COM (`wasapi::initialize_mta`), opens the default device
   for that direction, negotiates a 32-bit float format matching the device's native
   sample rate (`init_capture_stream`), starts the WASAPI stream.
2. Creates a `HeapRb<f32>` ring buffer (capacity: `config::capture::RING_BUFFER_CAPACITY`)
   split into producer/consumer halves.
3. `spawn_fill_thread` — OS thread that waits on WASAPI's event handle and drains
   packets into the ring buffer producer (`fill_ring_buffer` → `drain_device_buffer` →
   `read_next_packet` → `push_to_ring_buffer`).
4. `spawn_forward_thread` — OS thread that polls the ring buffer consumer in
   `CONSUMER_CHUNK_SIZE`-sized chunks and pushes `AudioEvent::RawCapture` into the
   tokio `mpsc` channel via `blocking_send`.

### `SendableDevice`

```rust
struct SendableDevice(OpenDevice);
unsafe impl Send for SendableDevice {}
unsafe impl Sync for SendableDevice {}
```

A manual `unsafe impl Send`/`Sync` wrapper — WASAPI's COM-based types aren't
`Send`/`Sync` by default, but the code's actual usage pattern (create on one thread,
move once into the fill thread, never touch from elsewhere) is safe in practice. This
is a deliberate escape hatch around the type system, not an oversight.

### Backpressure handling

If the ring buffer fills up faster than the forward thread drains it,
`push_to_ring_buffer` logs a dropped-sample count rather than blocking the fill thread
(which must stay responsive to the WASAPI event handle). This is a deliberate
audio-glitches-over-deadlocks tradeoff.

---

## 8. Component: Voice Activity Detection

`audio/vad.rs` combines a neural VAD model with a hand-rolled turn-boundary state
machine.

### `VoiceDetector` trait

```rust
pub trait VoiceDetector: Send {
    fn is_speech(&mut self, chunk: &[f32]) -> bool;
    fn reset(&mut self);
}
```

Abstracts the ML inference away from the state machine — `VadChannel` doesn't know or
care that the concrete implementation is Silero/ONNX.

### `SileroVad`

Wraps a shared `ort::Session` (`static SHARED_SESSION: LazyLock<Arc<Mutex<Session>>>`,
loaded once from the embedded `silero_vad.onnx` via `include_bytes!`). Each
`SileroVad` instance keeps its own recurrent `state: Vec<f32>` (128×2 floats) — Silero
is a stateful streaming model, so state must persist between chunks within a turn and
reset between turns.

`speech_probability` builds three ONNX input tensors (`audio`, `state`, `sr`) per call,
runs inference under the shared session's mutex, and extracts both the speech
probability and the updated recurrent state.

### `VadChannel` — the turn-boundary state machine

```rust
enum TurnState {
    Silence,
    Speech { chunk_count: usize, audio: Vec<i16>, start_ms: u128, hangover_left: usize },
}
```

Processes audio in fixed `CHUNK_SAMPLES` (512) windows. Transitions:

| From | `is_speech` | To |
|---|---|---|
| `Silence` | `true` | `Speech` (turn started) |
| `Silence` | `false` | `Silence` |
| `Speech` | `true` | `Speech` (accumulate, reset hangover) |
| `Speech` (hangover > 1) | `false` | `Speech` (accumulate, decrement hangover) |
| `Speech` (hangover ≤ 1) | `false` | `Silence` (turn ends — emitted if long enough) |

`MIN_SPEECH_CHUNKS` (3) filters out noise bursts — a "turn" shorter than this is
discarded, not emitted. `HANGOVER_CHUNKS` (35) is how many consecutive silent chunks
are tolerated before a turn is considered over — this absorbs natural pauses mid-sentence
without fragmenting one utterance into many turns.

The transition logic is implemented as pure functions (`speech_continued`,
`speech_in_hangover`) taking no `&self` — only the state-mutating side effects
(logging) go through methods that touch `self`.

**Format note:** `VadChannel::push` accepts `&[i16]` (matching what
`AudioNormalizer` produces for the STT sender) and internally converts back to `f32`
(`i16_chunk_to_f32`) before calling `VoiceDetector::is_speech`, since Silero expects
float input. This round-trip through 16-bit quantization exists only because the same
normalized buffer is shared between the STT sender and the VAD — it is not required by
either consumer individually.

---

## 9. Component: Speech-to-Text (Deepgram)

`stt/deepgram.rs` implements `SttSender` for a Deepgram streaming WebSocket connection,
with automatic reconnection.

### Connection architecture

```
DeepgramSender (handle held by AudioProcessor)
    │  audio_tx: mpsc::UnboundedSender<Vec<u8>>
    │  end_turn_tx: mpsc::UnboundedSender<()>
    ▼
DeepgramConnection::supervise (tokio task, loops forever)
    │  on each iteration: open_session() → DeepgramSession
    ▼
DeepgramSession::run (tokio::select! loop)
    ├── stream.next()      → incoming transcript fragments from Deepgram
    ├── audio_rx.recv()    → outgoing audio bytes → forwarded to sender_task
    └── end_turn_rx.recv() → flush accumulated text as TurnComplete
```

`sender_task` is a separate tokio task per session that owns the WebSocket's write
half (`SplitSink`) — this decouples "receiving audio to send" from "receiving
transcript messages," both of which the session's `select!` loop needs to service
concurrently without one blocking the other.

### Reconnection

If the WebSocket stream closes (`SessionOutcome::StreamClosed`), `supervise` loops back
and calls `open_session()` again. If the initial connection attempt fails, it retries
after a fixed 2-second sleep. The only way `supervise` actually exits
(`SessionOutcome::Done`) is if the *sender* half's channels close — i.e., the
`DeepgramSender` handle itself was dropped.

### Transcript accumulation

Deepgram sends multiple `is_final` fragments per turn as speech continues.
`accumulate()` concatenates them into a single string (space-separated); `on_end_turn`
takes ownership of the accumulated string (`std::mem::take`) and emits it as a
`TurnComplete`, resetting the buffer for the next turn.

### Wire protocol

Connects to `config::deepgram::WS_URL` (`nova-2` model, `linear16` encoding, 16kHz,
mono, Spanish, server-side endpointing at 300ms). Authenticates via an `Authorization:
Token {api_key}` header. Audio is sent as `Message::Binary` frames of raw little-endian
`i16` PCM bytes.

---

## 10. Component: RAG Engine

`ai/mod.rs` — `RagEngine` is the single owner of the embedding client, LLM client, and
vector store; it exposes one behavioral method, `answer()`.

```rust
pub struct RagEngine {
    embedder: Box<dyn Embedder>,
    llm: Box<dyn Llm>,
    store: VectorStore,
}
```

### `RagEngine::load(context: &str)`

Does the full one-time setup: constructs `VoyageEmbedder` and `GroqLlm` (both fallible
— read API keys from env), chunks the raw context string by newline
(`chunk_context`), embeds every chunk in one batched Voyage API call
(`embed_and_build_store`), and upserts each into a fresh `VectorStore`.

### `RagEngine::answer(question: &str)`

1. Embeds the question (single embedding, not batched).
2. `retrieve()` — cosine-similarity search against the store, `TOP_K` (6) results
   filtered to `score >= MIN_SCORE` (0.30).
3. Logs the retrieved chunks with their scores (`log_context`) — useful for debugging
   why a particular answer did or didn't use certain background.
4. `prompt::build()` — constructs a system prompt instructing the LLM to answer in 2-4
   bullet points, anchor in the candidate's background when relevant, and never
   fabricate background details not present in the retrieved context.
5. `GroqLlm::complete()` — sends to `llama-3.1-8b-instant`.

### `VectorStore` (`ai/vector_store.rs`)

```rust
pub struct VectorStore { entries: Vec<Entry> }
struct Entry { id: String, vector: [f32; EMBEDDING_DIMS], payload: String }
```

Fixed-size `[f32; 512]` vectors (Voyage's `voyage-3-lite` dimension) — `to_fixed`
converts the embedder's `Vec<f32>` and panics if the dimension doesn't match, since a
mismatch would indicate a fundamentally broken embedding call, not a recoverable
runtime condition. Search is brute-force cosine similarity over all entries
(`score_all` → `rank_by_score` → `take_top`) — appropriate given the store never holds
more than a few dozen entries (one per line of typed context).

### Embedder / LLM clients

Both `VoyageEmbedder` (`ai/embedder.rs`) and `GroqLlm` (`ai/llm.rs`) follow the same
shape: a trait (`Embedder`/`Llm`) for testability/swappability, a `LazyLock<Client>`
shared `reqwest` client, API key read from env at construction, and a
build-request/call-api/extract-response pipeline of small functions. `VoyageEmbedder`
supports true batching (`embed_batch`) used once at startup; `GroqLlm` only exposes
single-prompt completion, since each interviewer question is answered independently.

### `ai/dispatch.rs` — question filtering & fan-out

```rust
pub async fn run_ai(rx, rag_engine: Arc<RagEngine>, transcript: Arc<Mutex<Transcript>>)
```

Filters `TurnComplete` to `Speaker::System` only (`is_interviewer_question`) — the
assumption being that in a typical call setup, system loopback audio is the
interviewer's voice. Every qualifying turn spawns its own ephemeral task
(`answer_in_background`) so that one slow LLM call never delays detecting or answering
the next question.

---

## 11. Component: Transcript

`transcript.rs` is the single owner of `transcript.txt`, introduced specifically to fix
a prior design where two independent writers (`stt::run` and an AI-response writer)
opened the same file with different semantics, creating a race condition on startup
ordering.

```rust
pub struct Transcript { file: File }

impl Transcript {
    pub fn open(path: &str) -> Self { /* create+write+truncate, once */ }
    pub fn write_line(&mut self, line: &str) { /* print + write, log on error */ }
}
```

Shared as `Arc<Mutex<Transcript>>` between `stt::run` (writes `"[User]: ..."` /
`"[Interviewer]: ..."` lines) and `ai::dispatch::run_ai` (writes `"[AI] ..."` lines) —
two tokio tasks, potentially writing concurrently, synchronized by the `Mutex`. Each
caller formats its own line (no shared format imposed by `Transcript` itself), since the
two producers never had a shared line format to begin with.

---

## 12. Component: Hotkey / Pause Control

`audio/hotkey.rs` — a minimal, purely additive control surface: a global `F9` toggle
that pauses/resumes the entire pipeline without tearing anything down.

```rust
pub type PauseFlag = Arc<AtomicBool>;
```

`spawn_hotkey_listener` runs a dedicated OS thread polling
`GetAsyncKeyState(VK_F9)` every 30ms (edge-detected via a `was_pressed` bool, so a
single physical press toggles once, not repeatedly while held). Toggling is done with
`fetch_xor(true, Ordering::Relaxed)` — flips the flag and returns the previous value in
one atomic operation, avoiding a separate read-then-write race.

The flag is read (not written) inside `run_audio`'s main loop
(`audio/router.rs`) — when set, incoming `AudioEvent`s are dropped before reaching the
`AudioRouter`, so nothing downstream (VAD, STT, transcript) sees any activity while
paused.

---

## 13. Design Patterns Used

| Pattern | Where | Why |
|---|---|---|
| **Actor-style pipeline via channels** | `pipeline::start` wiring `audio_tx` → `turn_complete_tx` → `ai_tx` | Each stage (audio routing, STT relay, AI dispatch) is an independent tokio task; channels provide both communication and natural backpressure |
| **State machine (explicit enum + transition function)** | `VadChannel` / `TurnState` | Turn-boundary detection has genuinely distinct states (silence vs. speech-with-hangover) with different valid transitions — modeling it as data makes invalid states unrepresentable |
| **Trait objects for external dependencies** | `Box<dyn Embedder>`, `Box<dyn Llm>`, `Box<dyn SttSender>`, `Box<dyn VoiceDetector>` | Swappable providers (e.g., a mock STT sender for tests) without touching call sites |
| **Shared ownership via `Arc`, single-writer via `Mutex`** | `Arc<RagEngine>`, `Arc<Mutex<Transcript>>`, `PauseFlag = Arc<AtomicBool>` | `RagEngine` is read-only after construction (no `Mutex` needed); `Transcript` has genuine concurrent writers; `PauseFlag` is a single bool, so an atomic suffices over a full `Mutex` |
| **Supervisor / auto-reconnect loop** | `DeepgramConnection::supervise` | External WebSocket connections are inherently unreliable; the supervisor isolates reconnection logic from the rest of the pipeline, which never sees a dropped connection |
| **Bridging blocking OS APIs into async via channels (not `spawn_blocking`)** | `wasapi.rs`, `hotkey.rs` | Dedicated OS threads communicate outward via a lock-free ring buffer / atomic flag, rather than occupying a tokio blocking-pool thread indefinitely |
| **RAG (retrieval-augmented generation)** | `RagEngine` | Candidate background is embedded once, retrieved semantically per-question, keeping the LLM prompt small and relevant instead of stuffing the entire background into every call |

---

## 14. External Integrations & Protocols

| Service | Protocol | Purpose | Auth |
|---|---|---|---|
| **Deepgram** | WebSocket (`wss://api.deepgram.com/v1/listen`), binary `linear16` PCM frames in, JSON transcript events out | Real-time speech-to-text, one connection per speaker | `Authorization: Token {DEEPGRAM_API_KEY}` header |
| **Voyage AI** | HTTPS REST (`POST /v1/embeddings`), JSON | Text embeddings (`voyage-3-lite`, 512 dims) — both batch (startup) and single (per-question) | Bearer token (`VOYAGE_API_KEY`) |
| **Groq** | HTTPS REST (`POST /openai/v1/chat/completions`, OpenAI-compatible schema), JSON | LLM completion (`llama-3.1-8b-instant`) | Bearer token (`GROQ_API_KEY`) |
| **Windows WASAPI** | Native COM API (not network) | Microphone capture + system loopback capture | N/A (OS-level device permissions) |
| **ONNX Runtime** | In-process, embedded model | Silero VAD inference | N/A (model bundled in binary via `include_bytes!`) |

---

## 15. Configuration

`config.rs` centralizes every tunable constant, grouped by the subsystem that owns it:

| Module | Constants | Used by |
|---|---|---|
| `config::capture` | `RING_BUFFER_CAPACITY`, `CONSUMER_CHUNK_SIZE`, `EVENT_TIMEOUT_MS` | `audio/wasapi.rs` |
| `config::resampler` | `TARGET_SAMPLE_RATE` (16kHz), `INPUT_CHUNK_FRAMES`, `SUB_CHUNKS` | `audio/resampler.rs`, `audio/vad.rs` (for `duration_secs`) |
| `config::vad` | `CHUNK_SAMPLES`, `SPEECH_THRESHOLD`, `HANGOVER_CHUNKS`, `MIN_SPEECH_CHUNKS` | `audio/vad.rs` |
| `config::deepgram` | `WS_URL` (full query string: model, language, encoding, sample rate, endpointing) | `stt/deepgram.rs` |
| `config::ai` | `EMBEDDING_DIMS`, `TOP_K`, `MIN_SCORE` | `ai/vector_store.rs`, `ai/mod.rs` |
| `config::transcript` | `PATH` (`"transcript.txt"`) | `pipeline.rs` (passed into `Transcript::open`) |

### `Environment` (also in `config.rs`)

```rust
pub struct Environment { pub deepgram_api_key: String, pub pause_flag: PauseFlag }
```

Loaded once in `main.rs` via `Environment::load()` — reads `.env` (via `dotenvy`),
required env var `DEEPGRAM_API_KEY` (panics if missing), and constructs a fresh
`PauseFlag`. Note: `VOYAGE_API_KEY` and `GROQ_API_KEY` are **not** part of
`Environment` — they're read directly inside `VoyageEmbedder::new()` /
`GroqLlm::new()` at construction time, an inconsistency worth being aware of if you're
looking for "where do API keys come from" in one place.

---

## 16. Known Limitations & Planned Work

Documented here for completeness, not as criticism — these are open items identified
during the most recent refactor pass, not yet acted on:

- **No visual UI yet.** All interaction is console-based (`stdin` prompt at startup,
  `stdout` + `transcript.txt` during the session). A `ui/` module (`overlay`,
  `renderer`) is planned but not implemented.
- **`AudioRouter.conversation: Vec<SpeechTurn>`** (in `audio/router.rs`) is populated
  but never read — likely scaffolding for a future "give the LLM full conversation
  history" feature.
- **f32 → i16 → f32 round-trip** between `AudioNormalizer` and `VadChannel`: the
  normalizer quantizes to `i16` for the STT sender's benefit, and the VAD immediately
  converts back to `f32` for the ONNX model. Not a correctness bug, but a precision
  loss that a future pass could remove by having `DeepgramSender` quantize at the wire
  boundary instead.
- **`CaptureError { error: String }`** (in `audio/mod.rs`) is logged but never
  programmatically distinguished by failure type — fine today since nothing reacts
  differently to different capture failures.
- **Heavy use of `.unwrap()`/`.expect()`** in `audio/vad.rs` (ONNX tensor construction,
  session inference) means a malformed model output would panic the whole process
  rather than degrade gracefully for a single chunk.
- **API key loading is split** between `Environment` (Deepgram) and individual client
  constructors (Voyage, Groq) — see [§15](#15-configuration).