use crate::config;

pub struct SearchResult {
    pub payload: String,
    pub score:   f32,
}

struct Entry {
    id:      String,
    vector:  [f32; config::ai::EMBEDDING_DIMS],
    payload: String,
}

pub struct VectorStore {
    entries: Vec<Entry>,
}

impl VectorStore {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn upsert(&mut self, id: &str, vector: Vec<f32>, payload: &str) {
        let vector = to_fixed(vector);

        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(entry) => { entry.vector = vector; entry.payload = payload.to_string(); }
            None        => self.entries.push(Entry { id: id.to_string(), vector, payload: payload.to_string() }),
        }
    }

    pub fn search(&self, query: &[f32], top_k: usize) -> Vec<SearchResult> {
        let scored = self.score_all(query);
        let ranked = rank_by_score(scored);
        take_top(ranked, top_k)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl VectorStore {
    fn score_all(&self, query: &[f32]) -> Vec<SearchResult> {
        self.entries
            .iter()
            .map(|e| SearchResult {
                payload: e.payload.clone(),
                score:   cosine_similarity(&e.vector, query),
            })
            .collect()
    }
}

fn rank_by_score(mut results: Vec<SearchResult>) -> Vec<SearchResult> {
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results
}

fn take_top(results: Vec<SearchResult>, k: usize) -> Vec<SearchResult> {
    results.into_iter().take(k).collect()
}

fn to_fixed(v: Vec<f32>) -> [f32; config::ai::EMBEDDING_DIMS] {
    v.try_into()
        .unwrap_or_else(|_| panic!("vector debe tener exactamente {} dims", config::ai::EMBEDDING_DIMS))
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot    = a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    match (norm_a, norm_b) {
        (a, _) if a == 0.0 => 0.0,
        (_, b) if b == 0.0 => 0.0,
        (a, b)             => dot / (a * b),
    }
}
