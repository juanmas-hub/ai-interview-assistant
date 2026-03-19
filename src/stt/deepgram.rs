use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
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
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
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

impl DeepgramSession {
    async fn run(
        mut self,
        speaker:          Speaker,
        audio_rx:         &mut mpsc::UnboundedReceiver<Vec<u8>>,
        end_turn_rx:      &mut mpsc::UnboundedReceiver<()>,
        turn_complete_tx: &mpsc::Sender<TurnComplete>,
        accumulated:      &mut String,
    ) -> SessionOutcome {
        loop {
            tokio::select! {
                msg = self.stream.next() => {
                    if !self.on_ws_message(msg, accumulated, speaker) {
                        return SessionOutcome::StreamClosed;
                    }
                }
                audio = audio_rx.recv() => {
                    if !self.on_audio(audio) { return SessionOutcome::Done; }
                }
                end = end_turn_rx.recv() => {
                    if !self.on_end_turn(end, speaker, accumulated, turn_complete_tx).await {
                        return SessionOutcome::Done;
                    }
                }
            }
        }
    }

    fn on_ws_message(
        &self,
        msg:         Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
        accumulated: &mut String,
        speaker:     Speaker,
    ) -> bool {
        let msg = match msg {
            Some(Ok(m)) => m,
            _           => return false,
        };

        if let Some(fragment) = parse_is_final(msg) {
            accumulate(accumulated, &fragment, speaker);
        }

        true
    }

    fn on_audio(&self, audio: Option<Vec<u8>>) -> bool {
        match audio {
            Some(bytes) => { let _ = self.ws_tx.send(bytes); true }
            None        => false,
        }
    }

    async fn on_end_turn(
        &self,
        end:              Option<()>,
        speaker:          Speaker,
        accumulated:      &mut String,
        turn_complete_tx: &mpsc::Sender<TurnComplete>,
    ) -> bool {
        match end {
            Some(_) => {
                let text = std::mem::take(accumulated);
                let _ = turn_complete_tx.send(TurnComplete { speaker, text }).await;
                true
            }
            None => false,
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn accumulate(accumulated: &mut String, fragment: &str, speaker: Speaker) {
    if fragment.is_empty() { return; }
    if !accumulated.is_empty() { accumulated.push(' '); }
    accumulated.push_str(fragment);
    println!("[fragment] {speaker}: {fragment}");
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

fn parse_is_final(msg: Message) -> Option<String> {
    let text = msg.into_text().ok()?;
    let resp: DgResponse = serde_json::from_str(&text).ok()?;

    if resp.kind != "Results"            { return None; }
    if !resp.is_final.unwrap_or(false)   { return None; }

    resp.channel?
        .alternatives
        .into_iter()
        .next()
        .map(|a| a.transcript)
}

#[derive(Deserialize)]
struct DgResponse {
    #[serde(rename = "type")]
    kind:     String,
    is_final: Option<bool>,
    channel:  Option<DgChannel>,
}

#[derive(Deserialize)]
struct DgChannel {
    alternatives: Vec<DgAlternative>,
}

#[derive(Deserialize)]
struct DgAlternative {
    transcript: String,
}