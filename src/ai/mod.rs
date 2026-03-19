pub mod embedder;
pub mod vector_store;
mod llm;
mod prompt;

use anyhow::Result;
use std::sync::Arc;

use embedder::Embedder;
use llm::Llm;
use vector_store::{SearchResult, VectorStore};
use crate::config;
pub struct AiServices {
    pub embedder: Box<dyn Embedder>,
    pub llm:      Box<dyn Llm>,
}

impl AiServices {
    pub fn load() -> Result<Self> {
        Ok(Self {
            embedder: Box::new(embedder::VoyageEmbedder::new()?),
            llm:      Box::new(llm::GroqLlm::new()?),
        })
    }
}

pub async fn answer(
    question: &str,
    store:    &Arc<VectorStore>,
    services: &AiServices,
) -> Result<String> {
    let vector: Vec<f32>  = embed(question, &*services.embedder).await?;
    let context: Vec<SearchResult> = retrieve(store, &vector);
    let prompt: prompt::Prompt  = build_prompt(&context, question);

    log_context(&context);

    complete(prompt, &*services.llm).await
}

async fn embed(question: &str, embedder: &dyn Embedder) -> Result<Vec<f32>> {
    embedder.embed(question).await
}

fn retrieve(store: &Arc<VectorStore>, vector: &[f32]) -> Vec<SearchResult> {
    store.search(vector, config::ai::TOP_K)
        .into_iter()
        .filter(|r| r.score >= config::ai::MIN_SCORE)
        .collect()
}

fn build_prompt(context: &[SearchResult], question: &str) -> prompt::Prompt {
    prompt::build(context, question)
}

async fn complete(prompt: prompt::Prompt, llm: &dyn Llm) -> Result<String> {
    llm.complete(prompt).await
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn log_context(context: &[SearchResult]) {
    println!("[ai] {} chunks recuperados:", context.len());
    for r in context {
        println!("  score={:.3} — {}…", r.score, truncate(&r.payload, 60));
    }
}

fn truncate(s: &str, max: usize) -> &str {
    s.char_indices()
        .nth(max)
        .map(|(i, _)| &s[..i])
        .unwrap_or(s)
}
