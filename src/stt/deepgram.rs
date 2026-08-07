use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tokio_tungstenite::{
    connect_async,
    MaybeTlsStream, WebSocketStream,
    tungstenite::{client::IntoClientRequest, Message},
};
use futures_util::stream::{SplitSink, SplitStream};

use crate::audio::Speaker;
use crate::config;
use super::{SttSender, TurnComplete};

type WsSink   = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
type WsStream = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

pub struct DeepgramSender {
    audio_tx:    mpsc::UnboundedSender<Vec<u8>>,
    end_turn_tx: mpsc::UnboundedSender<()>,
}

impl DeepgramSender {
    pub async fn connect(
        speaker:          Speaker,
        turn_complete_tx: mpsc::Sender<TurnComplete>,
        api_key:          &str,
    ) -> Result<Self> {
        let (audio_tx,    audio_rx)    = mpsc::unbounded_channel::<Vec<u8>>();
        let (end_turn_tx, end_turn_rx) = mpsc::unbounded_channel::<()>();

        tokio::spawn(
            DeepgramConnection::new(speaker, api_key.to_string(), turn_complete_tx)
                .supervise(audio_rx, end_turn_rx)
        );

        println!("[deepgram] {speaker} connected");
        Ok(Self { audio_tx, end_turn_tx })
    }
}

impl SttSender for DeepgramSender {
    fn send_audio(&self, samples: &[i16]) {
        let bytes = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let _ = self.audio_tx.send(bytes);
    }

    fn end_turn(&self) {
        let _ = self.end_turn_tx.send(());
    }
}

struct DeepgramConnection {
    speaker:          Speaker,
    api_key:          String,
    turn_complete_tx: mpsc::Sender<TurnComplete>,
    accumulated:      String,
}

impl DeepgramConnection {
    fn new(speaker: Speaker, api_key: String, turn_complete_tx: mpsc::Sender<TurnComplete>) -> Self {
        Self { speaker, api_key, turn_complete_tx, accumulated: String::new() }
    }

    async fn supervise(
        mut self,
        mut audio_rx:    mpsc::UnboundedReceiver<Vec<u8>>,
        mut end_turn_rx: mpsc::UnboundedReceiver<()>,
    ) {
        loop {
            match self.open_session().await {
                Ok(session) => {
                    let outcome = session.run(
                        self.speaker,
                        &mut audio_rx,
                        &mut end_turn_rx,
                        &self.turn_complete_tx,
                        &mut self.accumulated,
                    ).await;

                    match outcome {
                        SessionOutcome::Done         => break,
                        SessionOutcome::StreamClosed => {
                            eprintln!("[deepgram] {} stream closed, reconnecting…", self.speaker);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[deepgram] {} reconnect failed: {e}, retrying in 2s…", self.speaker);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    async fn open_session(&self) -> Result<DeepgramSession> {
        let (sink, stream) = open_websocket(&self.api_key).await?;
        println!("[deepgram] {} connected", self.speaker);

        let (ws_tx, ws_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(sender_task(sink, ws_rx));

        Ok(DeepgramSession { stream, ws_tx })
    }
}

struct DeepgramSession {
    stream: WsStream,
    ws_tx:  mpsc::UnboundedSender<Vec<u8>>,
}

enum SessionOutcome {
    StreamClosed,
    Done,
}

enum WsOutcome {
    Continue,
    StreamClosed,
}

impl DeepgramSession {
    async fn run(
        mut self,
        speaker:          Speaker,
        audio_rx:         &mut mpsc::UnboundedReceiver<Vec<u8>>,
        end_turn_rx:      &mut mpsc::UnboundedReceiver<()>,
        turn_complete_tx: &mpsc::Sender<TurnComplete>,
        accumulated:      &mut String,
    ) -> SessionOutcome {
        let mut flush_deadline: Option<Instant> = None;

        loop {
            tokio::select! {
                msg = self.stream.next() => {
                    match self.on_ws_message(msg, accumulated, speaker, turn_complete_tx, &mut flush_deadline).await {
                        WsOutcome::Continue     => {}
                        WsOutcome::StreamClosed => return SessionOutcome::StreamClosed,
                    }
                }
                audio = audio_rx.recv() => {
                    if !self.on_audio(audio) { return SessionOutcome::Done; }
                }
                end = end_turn_rx.recv() => {
                    match end {
                        Some(_) => self.on_local_turn_end(&mut flush_deadline),
                        None    => return SessionOutcome::Done,
                    }
                }
                _ = sleep_until_opt(flush_deadline) => {
                    self.on_flush_timeout(accumulated, speaker, turn_complete_tx).await;
                    flush_deadline = None;
                }
            }
        }
    }

    async fn on_ws_message(
        &self,
        msg:              Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
        accumulated:      &mut String,
        speaker:          Speaker,
        turn_complete_tx: &mpsc::Sender<TurnComplete>,
        flush_deadline:   &mut Option<Instant>,
    ) -> WsOutcome {
        let msg = match msg {
            Some(Ok(m)) => m,
            _           => return WsOutcome::StreamClosed,
        };

        let Some(event) = parse_results_event(msg) else {
            return WsOutcome::Continue;
        };

        if let Some(fragment) = event.fragment {
            accumulate(accumulated, &fragment, speaker);
        }

        if event.speech_final {
            flush_turn(accumulated, speaker, turn_complete_tx).await;
            *flush_deadline = None; 
        }

        WsOutcome::Continue
    }

    fn on_local_turn_end(&self, flush_deadline: &mut Option<Instant>) {
        if flush_deadline.is_none() {
            *flush_deadline = Some(Instant::now() + Duration::from_millis(config::deepgram::FLUSH_GRACE_MS));
        }
    }

    async fn on_flush_timeout(
        &self,
        accumulated:      &mut String,
        speaker:          Speaker,
        turn_complete_tx: &mpsc::Sender<TurnComplete>,
    ) {
        if !accumulated.trim().is_empty() {
            eprintln!("[deepgram] {speaker} speech_final no llegó a tiempo — flush por fallback");
        }
        flush_turn(accumulated, speaker, turn_complete_tx).await;
    }

    fn on_audio(&self, audio: Option<Vec<u8>>) -> bool {
        match audio {
            Some(bytes) => { let _ = self.ws_tx.send(bytes); true }
            None        => false,
        }
    }
}

async fn sleep_until_opt(deadline: Option<Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None    => std::future::pending().await,
    }
}

fn accumulate(accumulated: &mut String, fragment: &str, speaker: Speaker) {
    if fragment.is_empty() { return; }
    if !accumulated.is_empty() { accumulated.push(' '); }
    accumulated.push_str(fragment);
    println!("[fragment] {speaker}: {fragment}");
}

async fn flush_turn(accumulated: &mut String, speaker: Speaker, turn_complete_tx: &mpsc::Sender<TurnComplete>) {
    if accumulated.is_empty() {
        return;
    }
    let text = std::mem::take(accumulated);
    let _ = turn_complete_tx.send(TurnComplete { speaker, text }).await;
}

async fn sender_task(mut sink: WsSink, mut rx: mpsc::UnboundedReceiver<Vec<u8>>) {
    while let Some(bytes) = rx.recv().await {
        if sink.send(Message::Binary(bytes)).await.is_err() { break; }
    }
}

async fn open_websocket(api_key: &str) -> Result<(WsSink, WsStream)> {
    let mut request = config::deepgram::WS_URL.into_client_request()?;
    request.headers_mut().insert(
        "Authorization",
        format!("Token {api_key}").parse()?,
    );
    let (ws, _) = connect_async(request).await?;
    Ok(ws.split())
}

struct ResultsEvent {
    fragment:     Option<String>,
    speech_final: bool,
}

fn parse_results_event(msg: Message) -> Option<ResultsEvent> {
    let text = msg.into_text().ok()?;
    let resp: DgResponse = serde_json::from_str(&text).ok()?;

    if resp.kind != "Results" {
        return None;
    }

    let speech_final = resp.speech_final.unwrap_or(false);
    let is_final     = resp.is_final.unwrap_or(false);

    let fragment = if is_final {
        resp.channel
            .and_then(|c| c.alternatives.into_iter().next())
            .map(|a| a.transcript)
            .filter(|t| !t.is_empty())
    } else {
        None
    };

    Some(ResultsEvent { fragment, speech_final })
}

#[derive(Deserialize)]
struct DgResponse {
    #[serde(rename = "type")]
    kind:         String,
    is_final:     Option<bool>,
    speech_final: Option<bool>,
    channel:      Option<DgChannel>,
}

#[derive(Deserialize)]
struct DgChannel {
    alternatives: Vec<DgAlternative>,
}

#[derive(Deserialize)]
struct DgAlternative {
    transcript: String,
}