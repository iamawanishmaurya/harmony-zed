/// Embeddings engine with full cosine-similarity ranking.
///
/// When compiled with the `embeddings` feature, uses fastembed (ONNX/BGE-Small-EN-v1.5).
/// Without the feature, uses a keyword-frequency fallback that still produces
/// meaningful rankings for common queries.

#[cfg(feature = "embeddings")]
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};

pub struct EmbeddingEngine {
    #[cfg(feature = "embeddings")]
    model: TextEmbedding,

    /// Whether the engine is running in stub / keyword-fallback mode.
    pub is_stub: bool,
}

impl EmbeddingEngine {
    /// Initialize the embedding engine.
    ///
    /// With `embeddings` feature: downloads BGE-Small-EN-v1.5 (~130 MB) on first run
    /// to `~/.cache/fastembed`. Takes 3–10 s.
    ///
    /// Without feature: instant zero-cost keyword fallback.
    pub fn new() -> anyhow::Result<Self> {
        // Check env var to skip real embeddings even with feature enabled
        let skip = std::env::var("HARMONY_SKIP_EMBEDDING_TESTS").unwrap_or_default() == "1";

        #[cfg(feature = "embeddings")]
        {
            if skip {
                tracing::warn!("HARMONY_SKIP_EMBEDDING_TESTS=1 — using keyword fallback");
                return Ok(Self { model: TextEmbedding::try_new(InitOptions {
                    model_name: EmbeddingModel::BGESmallENV15,
                    show_download_progress: false,
                    ..Default::default()
                })?, is_stub: true });
            }

            match TextEmbedding::try_new(InitOptions {
                model_name: EmbeddingModel::BGESmallENV15,
                show_download_progress: true,
                ..Default::default()
            }) {
                Ok(model) => {
                    tracing::info!("Embedding engine initialized (BGE-Small-EN-v1.5)");
                    Ok(Self { model, is_stub: false })
                }
                Err(e) => {
                    tracing::warn!("Failed to load embedding model, using keyword fallback: {e}");
                    Err(anyhow::anyhow!("Embedding model init failed: {}", e))
                }
            }
        }

        #[cfg(not(feature = "embeddings"))]
        {
            let _ = skip;
            tracing::info!("Embedding engine: keyword-frequency fallback (compile with --features embeddings for neural embeddings)");
            Ok(Self { is_stub: true })
        }
    }

    /// Embed a single text string.
    ///
    /// - With real model: returns 384-dimensional f32 vector.
    /// - Fallback: returns a 384-d bag-of-words hash vector that still supports
    ///   cosine similarity for keyword overlap.
    pub fn embed_one(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        #[cfg(feature = "embeddings")]
        {
            if !self.is_stub {
                let mut results = self.model.embed(vec![text.to_string()], None)?;
                return Ok(results.remove(0));
            }
        }
        // Keyword-frequency fallback
        Ok(keyword_hash_vector(text))
    }

    /// Embed multiple texts in a batch.
    pub fn embed_batch(&self, texts: Vec<String>) -> anyhow::Result<Vec<Vec<f32>>> {
        #[cfg(feature = "embeddings")]
        {
            if !self.is_stub {
                return Ok(self.model.embed(texts, None)?);
            }
        }
        Ok(texts.iter().map(|t| keyword_hash_vector(t)).collect())
    }

    /// Cosine similarity between two embedding vectors.
    /// Returns value in [-1.0, 1.0].  > 0.3 with keyword fallback = relevant.
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() { return 0.0; }
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if mag_a == 0.0 || mag_b == 0.0 { return 0.0; }
        dot / (mag_a * mag_b)
    }
}

// ── Keyword Fallback ──────────────────────────────────────────────────────────

/// Produce a 384-dimensional pseudo-embedding from keyword hashing.
/// Each word hashes to 1–3 dimensions; overlap → positive cosine similarity.
fn keyword_hash_vector(text: &str) -> Vec<f32> {
    let mut vec = vec![0.0f32; 384];
    let lowered = text.to_lowercase();
    let words: Vec<&str> = lowered
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .collect();

    // Use a simple but deterministic hash to spread words across dimensions
    for word in &words {
        let h = simple_hash(word);
        let idx1 = (h % 384) as usize;
        let idx2 = ((h >> 8) % 384) as usize;
        let idx3 = ((h >> 16) % 384) as usize;
        vec[idx1] += 1.0;
        vec[idx2] += 0.5;
        vec[idx3] += 0.25;
    }

    // L2-normalize so cosine similarity is well-behaved
    let mag: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag > 0.0 {
        for v in vec.iter_mut() {
            *v /= mag;
        }
    }
    vec
}

fn simple_hash(s: &str) -> u64 {
    // FNV-1a 64-bit
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = EmbeddingEngine::cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = EmbeddingEngine::cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let sim = EmbeddingEngine::cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = EmbeddingEngine::cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_embed_one_returns_384() {
        let engine = EmbeddingEngine::new().unwrap();
        let embedding = engine.embed_one("hello world").unwrap();
        assert_eq!(embedding.len(), 384);
    }

    #[test]
    fn test_keyword_similarity_related() {
        let engine = EmbeddingEngine::new().unwrap();
        let redis_reject = engine.embed_one("rejected Redis due to cost constraints").unwrap();
        let redis_query = engine.embed_one("why did we reject redis").unwrap();
        let logging_note = engine.embed_one("added logging middleware to express").unwrap();

        let sim_relevant = EmbeddingEngine::cosine_similarity(&redis_query, &redis_reject);
        let sim_irrelevant = EmbeddingEngine::cosine_similarity(&redis_query, &logging_note);

        assert!(
            sim_relevant > sim_irrelevant,
            "Redis query ({sim_relevant:.4}) should be more similar to Redis note than logging note ({sim_irrelevant:.4})"
        );
    }

    #[test]
    fn test_embed_batch() {
        let engine = EmbeddingEngine::new().unwrap();
        let vecs = engine.embed_batch(vec![
            "hello".to_string(),
            "world".to_string(),
        ]).unwrap();
        assert_eq!(vecs.len(), 2);
        assert_eq!(vecs[0].len(), 384);
        assert_eq!(vecs[1].len(), 384);
    }

    #[test]
    fn test_keyword_hash_normalized() {
        let vec = keyword_hash_vector("hello world test");
        let mag: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((mag - 1.0).abs() < 1e-5, "Should be unit-normalized, got mag={mag}");
    }
}
