pub mod dispatch;
pub mod embedder;
pub mod vector_store;
mod llm;
mod prompt;

use anyhow::Result;
use std::sync::Mutex;

use embedder::Embedder;
use llm::Llm;
use prompt::HistoryTurn;
use vector_store::{SearchResult, VectorStore};
use crate::config;

pub struct RagEngine {
    embedder: Box<dyn Embedder>,
    llm:      Box<dyn Llm>,
    store:    VectorStore,
    history:  Mutex<Vec<HistoryTurn>>,
}

impl RagEngine {
    pub async fn load(context: &str) -> Result<Self> {
        let embedder: Box<dyn Embedder> = Box::new(embedder::VoyageEmbedder::new()?);
        let llm:      Box<dyn Llm>      = Box::new(llm::GroqLlm::new()?);

        let chunks = chunk_context(context);
        let store  = embed_and_build_store(&chunks, &*embedder).await?;

        Ok(Self { embedder, llm, store, history: Mutex::new(Vec::new()) })
    }

    pub async fn answer(&self, question: &str) -> Result<String> {
        let vector  = self.embedder.embed(question).await?;
        let context = self.retrieve(&vector);
        self.log_context(&context);

        let history  = self.recent_history();
        let prompt   = prompt::build(&context, question, &history);
        let response = self.llm.complete(prompt).await?;

        self.record_turn(question, &response);

        Ok(response)
    }

    fn retrieve(&self, vector: &[f32]) -> Vec<SearchResult> {
        self.store
            .search(vector, config::ai::TOP_K)
            .into_iter()
            .filter(|r| r.score >= config::ai::MIN_SCORE)
            .collect()
    }

    fn recent_history(&self) -> Vec<HistoryTurn> {
        let history = self.history.lock().unwrap();
        let start   = history.len().saturating_sub(config::ai::MAX_HISTORY_TURNS);
        history[start..].to_vec()
    }

    fn record_turn(&self, question: &str, answer: &str) {
        self.history.lock().unwrap().push(HistoryTurn {
            question: question.to_string(),
            answer:   answer.to_string(),
        });
    }

    fn log_context(&self, context: &[SearchResult]) {
        println!("[ai] {} chunks recuperados:", context.len());
        for r in context {
            println!("  score={:.3} — {}…", r.score, truncate(&r.payload, 60));
        }
    }
}


fn chunk_context(context: &str) -> Vec<String> {
    context
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

async fn embed_and_build_store(chunks: &[String], embedder: &dyn Embedder) -> Result<VectorStore> {
    println!("[ai] vectorizando {} chunks…", chunks.len());
    let texts: Vec<&str> = chunks.iter().map(String::as_str).collect();
    let vectors = embedder.embed_batch(&texts).await?;

    let mut store = VectorStore::new();
    for (i, (payload, vector)) in chunks.iter().zip(vectors).enumerate() {
        let id = format!("ctx-{:03}", i);
        store.upsert(&id, vector, payload);
    }

    println!("[ai] store listo — {} chunks cargados", store.len());
    Ok(store)
}

fn truncate(s: &str, max: usize) -> &str {
    s.char_indices()
        .nth(max)
        .map(|(i, _)| &s[..i])
        .unwrap_or(s)
}