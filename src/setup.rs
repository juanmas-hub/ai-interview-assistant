use anyhow::Result;
use crate::ai::{embedder, vector_store::VectorStore};

pub async fn load(context: &str) -> Result<VectorStore> {
    let chunks  = chunk_context(context);
    let vectors = embed_chunks(&chunks).await?;
    let store   = build_store(&chunks, vectors);

    println!("[setup] store listo — {} chunks cargados", store.len());
    Ok(store)
}

fn chunk_context(context: &str) -> Vec<String> {
    context
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

async fn embed_chunks(chunks: &[String]) -> Result<Vec<Vec<f32>>> {
    println!("[setup] vectorizando {} chunks…", chunks.len());
    let texts: Vec<&str> = chunks.iter().map(String::as_str).collect();
    embedder::embed_batch(&texts).await
}

fn build_store(chunks: &[String], vectors: Vec<Vec<f32>>) -> VectorStore {
    let mut store = VectorStore::new();

    for (i, (payload, vector)) in chunks.iter().zip(vectors).enumerate() {
        let id = format!("ctx-{:03}", i);
        store.upsert(&id, vector, payload);
    }

    store
}