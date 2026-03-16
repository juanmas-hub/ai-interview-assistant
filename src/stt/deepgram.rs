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

        tokio::spawn(connection_supervisor(
            speaker,
            audio_rx,
            end_turn_rx,
            turn_complete_tx,
            api_key.to_string(),
        ));

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

async fn connection_supervisor(
    speaker:          Speaker,
    mut audio_rx:     mpsc::UnboundedReceiver<Vec<u8>>,
    mut end_turn_rx:  mpsc::UnboundedReceiver<()>,
    turn_complete_tx: mpsc::Sender<TurnComplete>,
    api_key:          String,
) {
    let mut accumulated = String::new();

    loop {
        let (sink, stream) = match open_websocket(&api_key).await {
            Ok(ws) => ws,
            Err(e) => {
                eprintln!("[deepgram] {speaker} reconnect failed: {e}, retrying in 2s…");
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                continue;
            }
        };

        println!("[deepgram] {speaker} connected");

        let (ws_tx, ws_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(sender_task(sink, ws_rx));

        let closed = run_session(
            stream,
            speaker,
            &mut audio_rx,
            &mut end_turn_rx,
            &ws_tx,
            &turn_complete_tx,
            &mut accumulated,
        ).await;

        if closed {
            eprintln!("[deepgram] {speaker} stream closed, reconnecting…");
        } else {
            break;
        }
    }
}

async fn run_session(
    mut stream:       WsStream,
    speaker:          Speaker,
    audio_rx:         &mut mpsc::UnboundedReceiver<Vec<u8>>,
    end_turn_rx:      &mut mpsc::UnboundedReceiver<()>,
    ws_tx:            &mpsc::UnboundedSender<Vec<u8>>,
    turn_complete_tx: &mpsc::Sender<TurnComplete>,
    accumulated:      &mut String,
) -> bool {
    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    Some(Ok(msg)) => {
                        if let Some(fragment) = parse_is_final(msg) {
                            if !fragment.is_empty() {
                                if !accumulated.is_empty() { accumulated.push(' '); }
                                accumulated.push_str(&fragment);
                                println!("[fragment] {speaker}: {fragment}");
                            }
                        }
                    }
                    _ => return true,
                }
            }
            audio = audio_rx.recv() => {
                match audio {
                    Some(bytes) => { let _ = ws_tx.send(bytes); }
                    None        => return false,
                }
            }
            end = end_turn_rx.recv() => {
                match end {
                    Some(_) => {
                        let text = std::mem::take(accumulated);
                        let _ = turn_complete_tx.send(TurnComplete { speaker, text }).await;
                    }
                    None => return false,
                }
            }
        }
    }
}

async fn sender_task(mut sink: WsSink, mut rx: mpsc::UnboundedReceiver<Vec<u8>>) {
    while let Some(bytes) = rx.recv().await {
        if sink.send(Message::Binary(bytes)).await.is_err() {
            break;
        }
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

    if resp.kind != "Results" { return None; }
    if !resp.is_final.unwrap_or(false) { return None; }

    resp.channel?
        .alternatives
        .into_iter()
        .next()
        .map(|a| a.transcript)
}