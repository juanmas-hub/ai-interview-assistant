# AI Interview Copilot: Architecture & System Documentation

## Table of Contents

- [AI Interview Copilot: Architecture \& System Documentation](#ai-interview-copilot-architecture--system-documentation)
  - [Table of Contents](#table-of-contents)
  - [1. System Overview](#1-system-overview)
  - [2. Tech Stack \& External Crates](#2-tech-stack--external-crates)
  - [3. Module Map](#3-module-map)
  - [4. Core Data Types](#4-core-data-types)
    - [`LineKind` / `TranscriptLine` (`transcript.rs`)](#linekind--transcriptline-transcriptrs)
    - [`HistoryTurn` (`ai/prompt.rs`, private module — re-used by `ai/mod.rs`)](#historyturn-aipromptrs-private-module--re-used-by-aimodrs)
  - [5. Threads \& Async Tasks](#5-threads--async-tasks)
    - [Why the split changed](#why-the-split-changed)
    - [Real OS threads (`std::thread::spawn`)](#real-os-threads-stdthreadspawn)
    - [Tokio tasks (inside the `pipeline-runtime` thread's runtime)](#tokio-tasks-inside-the-pipeline-runtime-threads-runtime)
    - [Startup handshake](#startup-handshake)
  - [6. End-to-End Data Flow](#6-end-to-end-data-flow)
  - [7. Component: Audio Capture (WASAPI)](#7-component-audio-capture-wasapi)
  - [8. Component: Voice Activity Detection](#8-component-voice-activity-detection)
  - [9. Component: Speech-to-Text (Deepgram)](#9-component-speech-to-text-deepgram)
    - [Current design: `speech_final` primary, local VAD as a bounded fallback](#current-design-speech_final-primary-local-vad-as-a-bounded-fallback)
    - [`sleep_until_opt`](#sleep_until_opt)
  - [10. Component: RAG Engine](#10-component-rag-engine)
    - [`RagEngine::answer(question)`](#ragengineanswerquestion)
    - [Prompt philosophy — a real reframing, not a tweak](#prompt-philosophy--a-real-reframing-not-a-tweak)
    - [Concurrency note](#concurrency-note)
  - [11. Component: Transcript](#11-component-transcript)
  - [12. Component: Overlay UI](#12-component-overlay-ui)
    - [`ui::run_blocking`](#uirun_blocking)
    - [`OverlayApp`](#overlayapp)
  - [13. Component: Hotkey / Pause Control](#13-component-hotkey--pause-control)
  - [14. Design Patterns Used](#14-design-patterns-used)
  - [15. External Integrations \& Protocols](#15-external-integrations--protocols)
  - [16. Configuration](#16-configuration)
  - [17. Known Limitations](#17-known-limitations)
  - [18. Possible Next Steps](#18-possible-next-steps)
    - [Observability](#observability)
    - [Reliability](#reliability)
    - [Containerization — honest assessment, not a straightforward "yes"](#containerization--honest-assessment-not-a-straightforward-yes)
    - [Scalability](#scalability)
    - [Persistence \& UX](#persistence--ux)
    - [Testing](#testing)

---

## 1. System Overview

AI Interview Copilot is a **real-time Windows desktop assistant** that listens to both
sides of an online technical interview (candidate's microphone + interviewer's system
audio via loopback), transcribes both streams concurrently, detects when the
interviewer finishes asking a question, and generates a concise, personalized answer
using retrieval-augmented generation (RAG) — now with genuine multi-turn conversation
memory — over the candidate's own background.

**Key properties:**

- **Concurrent dual-stream audio**: microphone and system loopback are captured,
  normalized, transcribed, and voice-activity-detected independently.
- **Real-time, streaming**: capture → STT → RAG is wired via async channels end to end.
- **Personalized but not limited by it**: the candidate's background is retrieved
  semantically per question, but the LLM is instructed to always answer fully and
  technically — the background personalizes the answer, it doesn't cap its scope.
- **Session-aware**: the AI remembers the last few interviewer questions and its own
  suggested answers, replayed as real conversation turns — not just isolated Q&A.
- **Visual, not just console**: a native window (egui/eframe) collects the initial
  context, shows a live, color-coded conversation preview, and exposes Pause/Resume and
  Close controls.

**Process/thread ownership at a glance** — this is the one architectural decision most
worth understanding before touching anything else:

```
Main thread (owned by winit/eframe — required on Windows)
    → runs the overlay window's event loop, blocking, for the whole app lifetime

"pipeline-runtime" thread (spawned from main)
    → owns its own tokio::runtime::Runtime
    → hosts every async task: audio routing, STT, RAG dispatch, Deepgram connections
```

These two threads talk to each other only through a `tokio::sync::oneshot` channel (the
initial context, sent once) and a couple of `Arc<...>` shared handles (`PauseFlag`,
`LiveTranscript`) — there is no other coupling between "the window" and "the pipeline."

---

## 2. Tech Stack & External Crates

| Crate | Purpose |
|---|---|
| `tokio` (`full`) | Async runtime for the pipeline thread |
| `anyhow` | Error propagation |
| `wasapi` | Mic + system loopback capture |
| `windows-sys` | F9 hotkey (`GetAsyncKeyState`) |
| `ringbuf` | Lock-free bridge between OS capture threads and the async world |
| `rubato` | Audio resampling (48kHz→16kHz) |
| `ort` | ONNX Runtime — runs the embedded Silero VAD model |
| `tokio-tungstenite` | Deepgram WebSocket client |
| `futures-util` | Stream/sink combinators for the WebSocket split |
| `serde` / `serde_json` | Deepgram, Voyage, Groq payloads |
| `reqwest` (`json`) | HTTP client — Voyage AI, Groq |
| `async-trait` | `async fn` in `Embedder`/`Llm`/`SttSender` traits |
| `dotenvy` | Loads `.env` for API keys |
| `hound` | WAV file I/O (`audio/wav_writer.rs`) |
| `eframe` / `egui` | Native overlay window — setup screen + live conversation preview |
| `mockall` (dev) | Mocking `SttSender` in `stt` unit tests |

Still a single binary crate, not a workspace.

---

## 3. Module Map

```
ai-interview-assistant (binary crate)
│
├── src/
│   ├── main.rs              # NOT async — owns the main thread for eframe/winit.
│   │                         # Spawns the pipeline-runtime thread, then blocks
│   │                         # running the overlay window.
│   │
│   ├── config.rs             # All tunable constants + Environment
│   ├── pipeline.rs           # Orchestrator: awaits initial context, wires channels,
│   │                         # spawns the pipeline's 3 top-level tokio tasks
│   ├── transcript.rs         # LineKind, TranscriptLine, LiveTranscript, Transcript
│   │
│   ├── audio/
│   │   ├── mod.rs            # Speaker, AudioFormat, AudioEvent
│   │   ├── router.rs         # AudioProcessor, AudioRouter, run_audio
│   │   ├── normalizer.rs     # AudioNormalizer — resample + downmix + quantize to i16
│   │   ├── resampler.rs      # Resampler — wraps rubato::FftFixedIn
│   │   ├── vad.rs            # VoiceDetector trait, SileroVad, VadChannel
│   │   ├── wasapi.rs         # OS-thread-based capture
│   │   ├── hotkey.rs         # F9 pause/resume listener (OS thread)
│   │   ├── wav_writer.rs     # (not covered)
│   │   └── silero_vad.onnx
│   │
│   ├── stt/
│   │   ├── mod.rs            # TurnComplete, SttSender trait, run()
│   │   └── deepgram.rs       # DeepgramSender — speech_final-driven, VAD-fallback flush
│   │
│   ├── ai/
│   │   ├── mod.rs            # RagEngine — embedder + llm + store + session history
│   │   ├── dispatch.rs       # run_ai — filters interviewer questions, dispatches
│   │   ├── embedder.rs       # Embedder trait, VoyageEmbedder
│   │   ├── llm.rs            # Llm trait, GroqLlm — builds full multi-turn message list
│   │   └── vector_store.rs   # VectorStore — in-memory cosine-similarity search
│   │   # ai/prompt.rs is private (mod prompt;) — HistoryTurn, Prompt, build()
│   │
│   └── ui/
│       ├── mod.rs            # run_blocking() — entry point, runs on the caller's thread
│       └── overlay.rs        # OverlayApp — eframe::App impl, setup + live preview
│
├── .env
└── Cargo.toml
```

> `ai/rag.rs` (an earlier, orphaned version of the RAG logic that predated `RagEngine`)
> was deleted — it wasn't referenced by any `mod` declaration and duplicated what
> `ai/mod.rs` now owns.

---

## 4. Core Data Types

Only what's new or changed since the last revision; unchanged types (`Speaker`,
`AudioFormat`, `AudioEvent`, `TurnComplete`, `SpeechTurn`, `SearchResult`) aren't
repeated here.

### `LineKind` / `TranscriptLine` (`transcript.rs`)

```rust
pub enum LineKind { User, Interviewer, Ai }

pub struct TranscriptLine { pub kind: LineKind, pub text: String }
```

Replaces what used to be pre-formatted `String` lines in the live transcript buffer.
The UI can now style a line by matching on `kind` instead of parsing a `"[AI] ..."`
prefix out of plain text — `Transcript::log()` is the single place that knows how a
line is labeled, both for the file and for the UI.

### `HistoryTurn` (`ai/prompt.rs`, private module — re-used by `ai/mod.rs`)

```rust
pub struct HistoryTurn { pub question: String, pub answer: String }
```

One (interviewer question, AI-suggested answer) pair. **Not** the candidate's own
spoken answer — see [§17](#17-known-limitations).

---

## 5. Threads & Async Tasks

The threading model changed substantially since the overlay UI was introduced — this
section supersedes the previous revision entirely.

### Why the split changed

`eframe`/`winit` require the event loop to run on the process's **main thread** on
Windows — this isn't a style preference, it's enforced (attempting otherwise panics:
*"Initializing the event loop outside of the main thread is a significant
cross-platform compatibility hazard"*). Since `main()` can't be `async` and own the
event loop at the same time, the async pipeline was moved to its **own thread with its
own `tokio::runtime::Runtime`**, leaving the main thread free for the window.

### Real OS threads (`std::thread::spawn`)

| Thread | Spawned in | Purpose |
|---|---|---|
| Main thread | process entry | Owns `eframe::run_native` — the overlay window's event loop, for the whole app lifetime |
| `pipeline-runtime` | `main.rs::spawn_pipeline` | Owns a dedicated `tokio::runtime::Runtime`; hosts every async task below |
| Fill / forward threads (×2 per audio source) | `wasapi::start_concurrent_capture` | Unchanged — see previous capture section |
| Hotkey listener | `Environment::start_hotkey_listener` | Unchanged — polls F9, toggles `PauseFlag` |

### Tokio tasks (inside the `pipeline-runtime` thread's runtime)

| Task | Spawned in | Lifetime |
|---|---|---|
| `run_pipeline` (awaits `pipeline::start`, then blocks on ctrl_c or forever) | `main.rs::spawn_pipeline` | Application lifetime — this is what keeps the runtime (and everything below) alive; without it, the runtime would drop and kill every spawned task the moment `pipeline::start` returns |
| `run_audio` | `pipeline::start` | Application lifetime |
| `stt::run` | `pipeline::start` | Application lifetime |
| `run_ai` (`ai::dispatch`) | `pipeline::start` | Application lifetime |
| `DeepgramConnection::supervise` (×2) | `DeepgramSender::connect` | Application lifetime |
| `sender_task` | Re-spawned on every Deepgram (re)connect | One per WebSocket session |
| `answer_in_background` | `ai::dispatch::run_ai`, per question | Ephemeral |

### Startup handshake

```
main thread                              pipeline-runtime thread
────────────                             ────────────────────────
Environment::load()
start_hotkey_listener()
create oneshot::channel()  ─────────────▶ spawn_pipeline(env, live_transcript, rx)
                                              rt.block_on(run_pipeline(...))
                                                pipeline::start(...)
                                                  context_rx.await  ◀── blocks here
ui::run_blocking(pause_flag,
    live_transcript, context_tx)
  → OverlayApp shows setup screen
  → user clicks "Start session"
  → context_tx.send(context)  ─────────────────▶ context_rx resolves
                                                  RagEngine::load(context).await
                                                  … rest of pipeline::start …
  → window stays open, now showing
    the live conversation preview
```

The oneshot is deliberately **one-shot** at the type level (`Sender::send` consumes
`self`) — the initial context can only ever be sent once, matching the fact that
`RagEngine::load` only runs once per session.

---

## 6. End-to-End Data Flow

The capture → normalize → VAD → Deepgram pipeline is unchanged from the previous
revision (see the diagram there if needed). What's new:

```
DeepgramSession (per speaker)
    │  event.speech_final == true  → flush immediately (primary path)
    │  OR local VAD end_turn() with no speech_final within FLUSH_GRACE_MS
    │     → flush anyway (fallback path, logged when it fires)
    ▼
TurnComplete { speaker, text }
    ▼
stt::run
    │  transcript.log(LineKind::User | LineKind::Interviewer, text)
    │     → writes to transcript.txt AND pushes a TranscriptLine to LiveTranscript
    ▼
run_ai (filters Speaker::System only)
    ▼
RagEngine::answer(question)
    1. embed(question) → Voyage
    2. retrieve() → VectorStore search, TOP_K filtered by MIN_SCORE
    3. recent_history() → last MAX_HISTORY_TURNS (question, answer) pairs
    4. prompt::build(context, question, history) → Prompt { system, history, question }
    5. GroqLlm::complete(prompt)
         → build_request() replays history as real user/assistant message pairs,
           THEN appends the current question — genuine multi-turn chat, not text
           stuffed into the system prompt
    6. record_turn(question, response) → pushed onto RagEngine's session history
    ▼
transcript.log(LineKind::Ai, response)
    → writes to transcript.txt AND pushes a TranscriptLine (kind: Ai) to LiveTranscript
    ▼
OverlayApp repaints (polling live_transcript every ~300ms)
    → AI lines rendered in a soft blue (Color32::from_rgb(122, 162, 247)),
      everything else in the default text color
```

---

## 7. Component: Audio Capture (WASAPI)

Unchanged from the previous revision — see `audio/wasapi.rs`. Not affected by any of
the recent work (Deepgram, RAG, or UI changes).

---

## 8. Component: Voice Activity Detection

Unchanged from the previous revision — see `audio/vad.rs`. `VadChannel::push` still
receives `i16` and round-trips to `f32` for Silero, which remains an open item (see
[§17](#17-known-limitations)).

---

## 9. Component: Speech-to-Text (Deepgram)

This component changed the most structurally. The old design flushed a turn's
transcript the moment the **local VAD** detected silence — this created a real race
condition (confirmed in production logs) where a late-arriving Deepgram fragment for
one utterance would get merged into the *next* utterance's turn, or a genuinely-spoken
turn would be silently dropped because its text hadn't arrived from Deepgram yet when
the local VAD closed the turn.

### Current design: `speech_final` primary, local VAD as a bounded fallback

```
DeepgramSession::run — tokio::select! over 4 branches:
  1. stream.next()      → incoming Deepgram messages
  2. audio_rx.recv()     → outgoing audio bytes to forward to the socket
  3. end_turn_rx.recv()  → local VAD says "turn probably ended"
  4. sleep_until_opt(flush_deadline) → the fallback timer, if one is armed
```

- **Primary path**: every `Results` message is parsed for both `is_final` (a text
  fragment) and `speech_final` (Deepgram's own endpointing decision, enabled via
  `config::deepgram::WS_URL`'s `endpointing=300`). When `speech_final` is `true`, the
  accumulated text is flushed as a `TurnComplete` **in the same message**, so there's no
  possible race with anything arriving out of order on a separate channel.
- **Fallback path**: the local VAD's `end_turn()` (still called from
  `AudioProcessor::process` in `audio/router.rs`) no longer flushes directly — it arms a
  `flush_deadline` of `config::deepgram::FLUSH_GRACE_MS` (500ms) from now, *if none is
  already armed*. If `speech_final` arrives before that deadline, the deadline is
  cancelled. If it doesn't, the fallback fires: flushes whatever's accumulated, and logs
  `"speech_final no llegó a tiempo — flush por fallback"` — this line is a real signal
  worth watching for; frequent fallback firings would suggest Deepgram's endpointing
  isn't closing turns reliably on the system-loopback channel specifically (this was
  observed happening at least once with a 6.6-second silence gap in testing).

### `sleep_until_opt`

```rust
async fn sleep_until_opt(deadline: Option<Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None    => std::future::pending().await,
    }
}
```

A small idiom worth knowing: this lets an `Option<Instant>` participate as a
`tokio::select!` branch that simply never fires when there's nothing to wait for,
without needing to conditionally include/exclude the branch itself.

---

## 10. Component: RAG Engine

`RagEngine` (`ai/mod.rs`) now carries session state across calls, not just static
dependencies.

```rust
pub struct RagEngine {
    embedder: Box<dyn Embedder>,
    llm:      Box<dyn Llm>,
    store:    VectorStore,
    history:  Mutex<Vec<HistoryTurn>>,
}
```

### `RagEngine::answer(question)`

1. Embed the question, retrieve top-K context above `MIN_SCORE` (unchanged).
2. `recent_history()` — takes the last `MAX_HISTORY_TURNS` entries from `history`
   (locks, clones the slice, drops the lock — never holds the `std::sync::Mutex` guard
   across an `.await`, which would fail to compile in an async fn).
3. `prompt::build(context, question, history)` — see below.
4. `GroqLlm::complete(prompt)` — sends the *actual conversation*, not a text blob.
5. `record_turn(question, response)` — appends to `history` for future calls.

### Prompt philosophy — a real reframing, not a tweak

The previous prompt treated retrieved background as the *ceiling* of what the answer
could cover ("ground your answer in the background... fill gaps with general
knowledge"). In practice this made the model give thin or evasive answers whenever a
question's topic wasn't explicitly present in a retrieved chunk — e.g. asked about
observability with only a "has Rust experience" chunk retrieved, it wouldn't commit to
a real technical answer about observability *in Rust*.

The current prompt inverts this with an explicit **PRIMARY RULE**: always give a
complete, technically strong answer regardless of whether the exact topic is in the
background; the background *personalizes*, it never *limits scope*. A separate
`anchor_rule` still governs how hard to lean on retrieved context (weave in multiple
entries when relevant, use a loosely-related entry as a bridge rather than ignoring it)
and a still-strict rule against inventing **specific** facts (company/project
names, metrics, dates) remains — but general technical knowledge is now explicitly
described as *always encouraged*, not just a fallback.

### Concurrency note

Since `RagEngine` is shared as `Arc<RagEngine>` and `ai::dispatch::run_ai` spawns one
task per question, two questions arriving close together can run `answer()`
concurrently. `history` is `Mutex`-protected, so there's no data race, but a second
question's `recent_history()` snapshot may not yet include the first question's answer
if the first hasn't finished. This is a deliberate tradeoff, not a bug: serializing
`answer()` calls would delay responding to a new question while an older LLM call is
still in flight, which is worse for a real-time tool.

---

## 11. Component: Transcript

`transcript.rs` now owns **structured** lines, not pre-formatted strings.

```rust
pub struct Transcript { file: File, live: LiveTranscript }

impl Transcript {
    pub fn open(path: &str, live: LiveTranscript) -> Self { ... }
    pub fn log(&mut self, kind: LineKind, text: &str) { ... }
}
```

`log()` is the single place that decides both the file's line format
(`"{label}: {text}\n"`, now consistent across all three `LineKind`s — the old `"[AI]
..."` format without a colon was unified to match `"[User]: ..."` /
`"[Interviewer]: ..."`) and what gets pushed into `LiveTranscript` for the UI. Both
`stt::run` and `ai::dispatch::run_ai` call `transcript.log(...)` directly — neither
formats a line string itself anymore, removing a duplicated formatting concern that
used to live in two different modules.

---

## 12. Component: Overlay UI

New since the last revision. `ui/mod.rs` + `ui/overlay.rs` (the earlier `overlay.rs`
/ `renderer.rs` / `egui_app.rs` three-way split collapsed to two files, since the
original split never reflected a real responsibility boundary — see
[§14](#14-design-patterns-used)).

### `ui::run_blocking`

```rust
pub fn run_blocking(pause_flag: PauseFlag, live_transcript: LiveTranscript, context_tx: oneshot::Sender<String>)
```

Thin entry point — builds `eframe::NativeOptions` and calls `eframe::run_native`,
blocking the calling thread (must be the main thread) until the window closes.

### `OverlayApp`

Two screens, switched on `self.started`:

- **Setup screen**: a multiline text box (empty by default, with a hint — the earlier
  version had a bug where a placeholder-*looking* string was actually pre-filled real
  content the user had to manually delete) and a "Start session" button.
  `start_session()` calls `self.context_tx.take()` — the `Option<Sender<String>>`
  wrapper is what makes "send exactly once" enforceable at the type level, regardless of
  how many times the button is clicked.
- **Session screen**: Pause/Resume (toggles the shared `PauseFlag` via `fetch_xor`),
  Close (sends `ViewportCommand::Close`), and a scrolling conversation preview that
  reads `LiveTranscript` and renders `LineKind::Ai` lines in a soft blue
  (`Color32::from_rgb(122, 162, 247)`) — chosen specifically to be visually distinct
  without reading as an alert/error color.

`ctx.request_repaint_after(Duration::from_millis(300))` keeps the window refreshing on
its own so new transcript lines appear without requiring user interaction — the
transcript is being written from a completely different thread (the pipeline runtime),
so the window has no other way to know new lines exist.

---

## 13. Component: Hotkey / Pause Control

Unchanged in implementation, but now has **two independent callers** toggling the same
`PauseFlag`: the F9 OS-level listener (unchanged) and the overlay's Pause/Resume
button. Both are clones of the same `Arc<AtomicBool>`, so either one flipping it should
be visible to `run_audio`'s check in `audio/router.rs`. **This interaction currently has
an open, unconfirmed bug** — see [§17](#17-known-limitations).

---

## 14. Design Patterns Used

Only new/changed entries since the last revision:

| Pattern | Where | Why |
|---|---|---|
| **Dedicated OS thread for a blocking event loop, with the async runtime moved elsewhere** | `main.rs` (window) vs. `pipeline-runtime` thread (tokio) | winit's Windows-only constraint (main thread must own the event loop) meant flipping which side gets a dedicated thread, rather than trying to force the GUI off-thread |
| **One-shot handshake for a single startup value** | `oneshot::channel()` for the initial context | The value is fundamentally single-use (`RagEngine::load` runs once); `Sender::send(self, ...)` consuming the sender makes double-sending a compile error, not a runtime bug to guard against |
| **Structured live state instead of pre-formatted display strings** | `TranscriptLine { kind, text }` replacing plain `String` | Keeps "how do we format a line" in one place (`Transcript::log`) instead of duplicating it in the module that produces the line and the module that displays it |
| **Session state behind a `Mutex`, snapshotted before any `.await`** | `RagEngine.history` | `std::sync::MutexGuard` isn't `Send` across await points — `recent_history()` locks, clones, and drops the guard before any async work happens |
| **Bounded fallback timer racing against a primary signal** | `DeepgramSession`'s `speech_final` vs. `flush_deadline` | Keeps the "authoritative" decision (Deepgram's own endpointing) as the fast path while still bounding the failure mode (turn never closes) to a fixed, small window instead of leaving it unbounded |
| **Explicit reframing of an LLM instruction's priority ordering** | `ai/prompt.rs`'s `PRIMARY RULE` vs. `anchor_rule` | Discovered empirically that instructing "use the background, fill gaps with general knowledge" produced thin answers when the background didn't cover the topic — inverting which instruction is primary fixed it without touching retrieval at all |

---

## 15. External Integrations & Protocols

Unchanged table from the previous revision, with one behavioral note: Groq now
receives the **full conversation** (system + alternating user/assistant messages for
each history turn + the current question) on every call, not a single system+user pair
— this increases the token count per request roughly linearly with
`config::ai::MAX_HISTORY_TURNS`, worth keeping in mind if Groq costs or rate limits
ever become a concern.

---

## 16. Configuration

New/changed constants since the last revision:

| Module | Constant | Purpose |
|---|---|---|
| `config::deepgram` | `FLUSH_GRACE_MS` (500) | How long the fallback flush waits for `speech_final` before firing anyway |
| `config::ai` | `MAX_HISTORY_TURNS` (5) | How many (question, answer) pairs are replayed to the LLM per call |

Everything else (`capture`, `resampler`, `vad`, `ai::{EMBEDDING_DIMS,TOP_K,MIN_SCORE}`,
`transcript::PATH`, `Environment`) is unchanged.

---

## 17. Known Limitations

Carried over from the previous revision where still open, plus new items from this
round of work:

- **Pause/Resume bug under active investigation.** The Pause button visibly stops
  processing; Resume has been reported to not restore it. A diagnostic log was added to
  `audio/router.rs` (`"[audio] pause_flag visto por run_audio: {is_paused}"`, printed
  only on transition) to determine whether the flag itself fails to propagate back to
  `false`, or whether something downstream (VAD state, a Deepgram session) is stuck for
  an unrelated reason. **Root cause not yet confirmed as of this revision.**
- **History captures the AI's suggested answer, not the candidate's actual spoken
  answer.** `RagEngine.history` only ever records `(interviewer_question,
  ai_suggested_answer)` — the candidate's own `Speaker::User` turns are transcribed and
  shown, but never fed back into the LLM's conversation context. If the candidate
  answers differently than suggested, later questions won't know that.
- **f32 → i16 → f32 round-trip** between `AudioNormalizer` and `VadChannel` — still
  unaddressed; a precision-loss issue, not a correctness bug.
- **Heavy `.unwrap()`/`.expect()` usage in `audio/vad.rs`** (ONNX tensor construction,
  inference) — a malformed model output still panics the whole process.
- **`AudioRouter.conversation: Vec<SpeechTurn>`** — still populated, still unused.
- **API key loading is split** between `Environment` (Deepgram) and individual client
  constructors (Voyage, Groq).
- **No automated tests for `RagEngine.answer`'s history/retrieval logic** — `stt` and
  `ai::dispatch` have unit tests for transcript logging and turn filtering, but the
  session-history behavior (bounding to `MAX_HISTORY_TURNS`, correct ordering) is
  currently only verified manually.

---

## 18. Possible Next Steps

Organized by theme, roughly in the order they'd likely pay off given the project's
current state (a single-user Windows desktop tool, not a hosted service).

### Observability

- Replace `println!`/`eprintln!` with the `tracing` crate — structured, leveled logs
  (`debug!`/`info!`/`warn!`/`error!`) with fields instead of string interpolation,
  which would make the Deepgram fallback-flush frequency, VAD state transitions, and
  RAG retrieval scores all filterable/queryable instead of scrollback-only.
- A per-session correlation ID (e.g. one UUID generated at `RagEngine::load` time)
  threaded through every log line — useful the moment you're comparing behavior across
  multiple practice sessions.
- Lightweight metrics even just printed at shutdown: questions answered, average
  Deepgram-fallback rate, average Groq/Voyage latency, VAD turns discarded as noise
  bursts. Doesn't need a metrics backend (Prometheus etc.) for a single-user tool —
  just needs to exist somewhere other than "scroll up in the terminal."

### Reliability

- Resolve the pause/resume bug (see [§17](#17-known-limitations)) — highest-priority
  correctness item currently open.
- Retry/backoff on Voyage/Groq HTTP failures — today a single failed call just logs
  `[ai] error: ...` and the question goes unanswered; a transient network blip
  shouldn't cost an entire interview question.
- Feed the candidate's actual spoken answer into `RagEngine.history`, not just the
  AI's suggestion (see [§17](#17-known-limitations)) — probably the single highest-value
  change to answer quality on multi-question exchanges.

### Containerization — honest assessment, not a straightforward "yes"

This one deserves a real answer rather than a reflexive Dockerfile. **The app as a
whole can't reasonably run in a container**, for two structural reasons:

1. It links directly against Win32 APIs (`wasapi`, `windows-sys`/`GetAsyncKeyState`) —
   it can only run in a **Windows container**, not Linux, ruling out the usual
   Docker-for-Linux-containers tooling.
2. Even in a Windows container: **audio device passthrough isn't a supported Docker
   feature on Windows** the way it is on Linux (no equivalent of bind-mounting
   `/dev/snd` or `pulseaudio` sockets) — there's no standard way to expose the host's
   microphone/loopback devices to a Windows container. Same problem for the `eframe`
   window itself: Windows Server Core / Nano Server containers don't have a compositor
   for a native window to render into.

**What actually is containerizable**: the parts of `ai/` that don't touch Windows APIs
at all — `vector_store.rs`, `prompt.rs`, the `Embedder`/`Llm` trait definitions — have
zero Windows dependencies. A genuinely useful next step here isn't "containerize the
app," it's:

- **Split the RAG/LLM logic into a small headless service** (HTTP or gRPC) that the
  Windows client talks to over the network, instead of calling `RagEngine` in-process.
  That service — no WASAPI, no `eframe`, no `windows-sys` — containerizes cleanly on
  Linux, and would let you iterate on prompt/retrieval logic, add logging/metrics
  infrastructure, or even serve multiple client installs from one place, without
  touching the Windows-specific client at all.
- Short of that: a **Dockerfile just for CI**, running `cargo test` against the
  platform-independent modules on Linux, so tests for `vector_store`/`prompt` logic run
  in a reproducible container even though the shippable binary never runs in one.

### Scalability

For a single-user local tool, "scalability" in the server sense (concurrent users,
QPS) doesn't really apply. What *would* matter as the project grows:

- **Bounding growth over a long session** — `VectorStore` and `transcript.txt` both
  grow unboundedly with a very long interview; `RagEngine.history` is already bounded
  (`MAX_HISTORY_TURNS`), the others currently aren't, though in practice a single
  interview's context/transcript size is small enough that this is more of a
  "eventually" concern than an urgent one.
- If the RAG/LLM split above ever happens, *that's* the point where real scalability
  questions (connection pooling, concurrent request handling, maybe caching repeated
  embeddings) start to matter — deferring that discussion until the split exists is
  reasonable rather than solving it prematurely on the current architecture.

### Persistence & UX

- **A simple local profile, not a login.** Since this is a single-user desktop tool
  with no backend, "login" is the wrong frame — what's actually wanted is persisting
  the typed context locally (e.g. a JSON file in `%APPDATA%`) so it's pre-filled (and
  editable) on the next launch instead of retyped every session. A real login/auth
  concept would only start making sense *if* the RAG service split above happens and
  multiple client installs need to identify which candidate's profile to load — worth
  deferring until then rather than building auth for a single local user today.
- **Multiple named profiles** — since interview prep context can differ by role (e.g. a
  backend-heavy pitch vs. a full-stack one), letting the setup screen save/load a few
  named contexts would be a natural extension of the above.
- **Post-interview export** — `transcript.txt` already has everything; a "save as
  Markdown/PDF with timestamps" button on session close would turn it into something
  worth reviewing afterward, rather than a debug artifact.

### Testing

- Unit tests for `RagEngine::answer`'s history bounding and ordering (mock
  `Embedder`/`Llm` the same way `stt`'s tests mock `SttSender` with `mockall`).
- A test asserting `flush_turn`'s empty-accumulated-text guard and the
  `flush_deadline` cancel-on-`speech_final` logic in `stt/deepgram.rs`, since that's the
  most failure-prone piece of logic added recently and currently has zero test coverage
  of its own (only observed correct via manual log inspection).