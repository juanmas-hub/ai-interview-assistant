use std::fs::File;
use std::io::Write;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    User,
    Interviewer,
    Ai,
}

impl LineKind {
    fn label(self) -> &'static str {
        match self {
            LineKind::User        => "[User]",
            LineKind::Interviewer => "[Interviewer]",
            LineKind::Ai          => "[AI]",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TranscriptLine {
    pub kind: LineKind,
    pub text: String,
}

impl std::fmt::Display for TranscriptLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind.label(), self.text)
    }
}

pub type LiveTranscript = Arc<Mutex<Vec<TranscriptLine>>>;

pub fn new_live_transcript() -> LiveTranscript {
    Arc::new(Mutex::new(Vec::new()))
}

pub struct Transcript {
    file: File,
    live: LiveTranscript,
}

impl Transcript {
    pub fn open(path: &str, live: LiveTranscript) -> Self {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .unwrap_or_else(|e| panic!("failed to open {path}: {e}"));

        Self { file, live }
    }

    pub fn log(&mut self, kind: LineKind, text: &str) {
        let text = text.trim();
        let line = format!("{}: {text}\n", kind.label());

        print!("{line}");

        self.live.lock().unwrap().push(TranscriptLine {
            kind,
            text: text.to_string(),
        });

        if let Err(e) = self.file.write_all(line.as_bytes()) {
            eprintln!("[transcript] write error: {e}");
        }
    }
}