use anyhow::Result;
use std::sync::Arc;

use crate::ai::{embedder, llm, prompt};
use crate::ai::vector_store::{SearchResult, VectorStore};
use crate::config;

pub async fn answer(question: &str, store: &Arc<VectorStore>) -> Result<String> {
    let vector  = embed(question).await?;
    let context = search(store, &vector);
    let prompt  = build_prompt(&context, question);

    log_context(&context);

    complete(prompt).await
}

async fn embed(question: &str) -> Result<Vec<f32>> {
    embedder::embed(question).await
}

fn search(store: &Arc<VectorStore>, vector: &[f32]) -> Vec<SearchResult> {
    store.search(vector, config::ai::TOP_K)
        .into_iter()
        .filter(|r| r.score >= config::ai::MIN_SCORE)
        .collect()
}

fn build_prompt(context: &[SearchResult], question: &str) -> prompt::Prompt {
    prompt::build(context, question)
}

async fn complete(prompt: prompt::Prompt) -> Result<String> {
    llm::complete(prompt).await
}

fn log_context(context: &[SearchResult]) {
    println!("[rag] {} chunks recuperados:", context.len());
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