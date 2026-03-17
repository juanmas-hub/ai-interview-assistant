pub mod rag;
pub mod llm;

use anyhow::Result;

pub async fn run(question: &str) -> Result<String> {
    println!("[ai] pregunta recibida: «{question}»");
    Ok(format!("(respuesta pendiente para: «{question}»)"))
}