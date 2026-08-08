use crate::ai::vector_store::SearchResult;

#[derive(Debug, Clone)]
pub struct HistoryTurn {
    pub question: String,
    pub answer:   String,
}

pub struct Prompt {
    pub system:   String,
    pub history:  Vec<HistoryTurn>,
    pub question: String,
}

pub fn build(context: &[SearchResult], question: &str, history: &[HistoryTurn]) -> Prompt {
    Prompt {
        system:   build_system_prompt(context),
        history:  history.to_vec(),
        question: question.to_string(),
    }
}

fn build_system_prompt(context: &[SearchResult]) -> String {
    let anchor_rule      = anchor_rule(context);
    let context_section  = format_context_section(context);

    format!(
        "You are a technical interview copilot. The interviewer just asked the \
         candidate a question in real time — you have seconds, not minutes, to help.\n\
         \n\
         Respond with 4-6 bullet points the candidate can glance at and say out loud. \
         Each bullet should be one full sentence — enough to convey the what AND the \
         why/how, not a bare keyword fragment. Don't pad artificially: if the question \
         genuinely only supports 3 solid points, give 3 rather than stretching to 6 — \
         but default to giving real, usable signal over a thin list.\n\
         \n\
         PRIMARY RULE: Always give a complete, technically strong, specific answer to \
         the question — the kind a senior engineer with real hands-on experience would \
         give — regardless of whether the exact topic is covered in the candidate's \
         background below. The background personalizes the answer; it never limits its \
         scope. If the question is about something the background doesn't mention at \
         all, still answer it fully and concretely using genuine technical knowledge — \
         never give a thin or evasive answer just because there's no personal anchor \
         for it.\n\
         \n\
         {anchor_rule}\n\
         \n\
         Never invent a SPECIFIC fact that isn't in the background below — a company \
         name, a project name, a metric, a date. Speaking generally about real \
         technical practices is not invention and is always encouraged; inventing \
         personal specifics is never allowed.\n\
         \n\
         If earlier questions and answers from this same interview appear before this \
         message, build on them naturally — don't repeat a point you already made, and \
         reference something you said earlier if it's genuinely relevant.\n\
         \n\
         General rules:\n\
         - Lead with the most relevant point first.\n\
         - No preamble, no follow-up questions, no restating the question.\n\
         - Prefer concrete nouns (technology names, numbers, tradeoffs) over vague \
           adjectives.\n\
         {context_section}"
    )
}

fn anchor_rule(context: &[SearchResult]) -> String {
    if context.is_empty() {
        "No specific candidate background matched this question — that's fine, answer \
         from general expertise as described in the PRIMARY RULE above."
            .to_string()
    } else {
        format!(
            "The candidate background below may relate directly or only loosely to \
             this question — use it as a real anchor wherever it fits, up to {} \
             entries are provided so you can weave in more than one when relevant. If \
             an entry only loosely relates (e.g. the same language or ecosystem but a \
             different topic), use it as a natural bridge into the technical answer \
             rather than ignoring it — don't restrict the answer's scope to only what \
             these entries literally say.",
            context.len(),
        )
    }
}

fn format_context_section(context: &[SearchResult]) -> String {
    if context.is_empty() {
        return String::new();
    }

    let entries = context
        .iter()
        .enumerate()
        .map(|(i, r)| format!("[{}] {}", i + 1, r.payload))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!("\nCandidate background (most relevant first):\n{entries}\n")
}