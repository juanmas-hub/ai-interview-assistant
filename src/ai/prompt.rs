use crate::ai::vector_store::SearchResult;

pub struct Prompt {
    pub system: String,
    pub user:   String,
}

pub fn build(context: &[SearchResult], question: &str) -> Prompt {
    Prompt {
        system: build_system_prompt(context),
        user:   question.to_string(),
    }
}

fn build_system_prompt(context: &[SearchResult]) -> String {
    let context_block = format_context(context);

    format!(
        "Sos un asistente experto ayudando a un candidato en una entrevista técnica backend.\n\
         Respondé de forma clara y concisa en español.\n\
         Basate ÚNICAMENTE en el siguiente contexto técnico para responder:\n\n\
         {context_block}"
    )
}

fn format_context(context: &[SearchResult]) -> String {
    context
        .iter()
        .enumerate()
        .map(|(i, r)| format!("[{}] {}", i + 1, r.payload))
        .collect::<Vec<_>>()
        .join("\n\n")
}