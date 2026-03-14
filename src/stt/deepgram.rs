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
use super::{SttSender, TranscriptEvent};

type WsSink   = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
type WsStream = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

#[derive(Deserialize)]
struct DgResponse {
    #[serde(rename = "type")]
    kind:         String,
    // is_final:     Option<bool>,
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

pub struct DeepgramSender {
    audio_tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl DeepgramSender {
    pub async fn connect(
        speaker:       Speaker,
        transcript_tx: mpsc::Sender<TranscriptEvent>,
        api_key:       &str,
    ) -> Result<Self> {
        let (sink, stream) = open_websocket(api_key).await?;
        let (audio_tx, audio_rx) = mpsc::unbounded_channel();

        tokio::spawn(sender_task(sink, audio_rx));
        tokio::spawn(receiver_task(stream, speaker, transcript_tx));

        println!("[deepgram] {speaker} connected");
        Ok(Self { audio_tx })
    }
}

impl SttSender for DeepgramSender {
    fn send(&self, samples: &[i16]) {
        let bytes = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let _ = self.audio_tx.send(bytes);
    }
}


async fn sender_task(mut sink: WsSink, mut rx: mpsc::UnboundedReceiver<Vec<u8>>) {
    while let Some(bytes) = rx.recv().await {
        if sink.send(Message::Binary(bytes)).await.is_err() {
            break;
        }
    }
}

async fn receiver_task(
    mut stream:    WsStream,
    speaker:       Speaker,
    transcript_tx: mpsc::Sender<TranscriptEvent>,
) {
    while let Some(Ok(msg)) = stream.next().await {
        if let Some(event) = parse_message(msg, speaker) {
            let _ = transcript_tx.send(event).await;
        }
    }
    eprintln!("[deepgram] {speaker} stream closed");
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

fn parse_message(msg: Message, speaker: Speaker) -> Option<TranscriptEvent> {
    let text = msg.into_text().ok()?;
    let resp: DgResponse = serde_json::from_str(&text).ok()?;

    if resp.kind != "Results" {
        return None;
    }

    let transcript = resp.channel?.alternatives.into_iter().next()?.transcript;

    Some(TranscriptEvent {
        speaker,
        text:         transcript,
        // is_final:     resp.is_final.unwrap_or(false),
        speech_final: resp.speech_final.unwrap_or(false),
    })
}