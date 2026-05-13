//! Local ONNX-based embedding service — runs sentence-transformer models
//! without any external server. Privacy-first: no network calls, no API keys.
//!
//! Downloads the model on first use to `~/.config/omegon/models/<model>/`.
//! Default model: all-MiniLM-L6-v2 (22M params, 384-dim, ~80MB ONNX).

use async_trait::async_trait;
use omegon_memory::embedding::{EmbedError, EmbeddingService};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const DEFAULT_MODEL: &str = "all-MiniLM-L6-v2";
const DEFAULT_DIMS: usize = 384;

pub struct LocalEmbeddingService {
    session: Arc<Mutex<ort::session::Session>>,
    tokenizer: Arc<tokenizers::Tokenizer>,
    model_name: String,
}

impl LocalEmbeddingService {
    pub fn load(model_dir: &Path, model_name: &str) -> Result<Self, EmbedError> {
        let model_path = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");

        if !model_path.exists() || !tokenizer_path.exists() {
            return Err(EmbedError::Unavailable(format!(
                "model files not found at {} — run `omegon embedding download` or set OMEGON_EMBED_MODEL_DIR",
                model_dir.display()
            )));
        }

        let session = ort::session::Session::builder()
            .map_err(|e| EmbedError::Unavailable(format!("session builder: {e}")))?
            .with_intra_threads(1)
            .map_err(|e| EmbedError::Unavailable(format!("set threads: {e}")))?
            .commit_from_file(&model_path)
            .map_err(|e| EmbedError::Unavailable(format!("failed to load ONNX model: {e}")))?;

        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| EmbedError::Unavailable(format!("failed to load tokenizer: {e}")))?;

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            tokenizer: Arc::new(tokenizer),
            model_name: model_name.to_string(),
        })
    }

    pub fn from_default_dir() -> Result<Self, EmbedError> {
        let model_name =
            std::env::var("OMEGON_EMBED_LOCAL_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let model_dir = resolve_model_dir(&model_name);
        Self::load(&model_dir, &model_name)
    }

    fn embed_sync(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| EmbedError::RequestFailed(format!("tokenization failed: {e}")))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        let token_type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|&t| t as i64).collect();
        let seq_len = input_ids.len();

        let ids_array = ndarray::Array2::from_shape_vec((1, seq_len), input_ids)
            .map_err(|e| EmbedError::RequestFailed(format!("shape error: {e}")))?;
        let mask_array = ndarray::Array2::from_shape_vec((1, seq_len), attention_mask.clone())
            .map_err(|e| EmbedError::RequestFailed(format!("shape error: {e}")))?;
        let type_array = ndarray::Array2::from_shape_vec((1, seq_len), token_type_ids)
            .map_err(|e| EmbedError::RequestFailed(format!("shape error: {e}")))?;

        let ids_tensor = ort::value::TensorRef::from_array_view(&ids_array)
            .map_err(|e| EmbedError::RequestFailed(format!("tensor: {e}")))?;
        let mask_tensor = ort::value::TensorRef::from_array_view(&mask_array)
            .map_err(|e| EmbedError::RequestFailed(format!("tensor: {e}")))?;
        let type_tensor = ort::value::TensorRef::from_array_view(&type_array)
            .map_err(|e| EmbedError::RequestFailed(format!("tensor: {e}")))?;

        let mut session = self
            .session
            .lock()
            .map_err(|e| EmbedError::RequestFailed(format!("session lock: {e}")))?;
        let outputs = session
            .run(ort::inputs![ids_tensor, mask_tensor, type_tensor])
            .map_err(|e| EmbedError::RequestFailed(format!("inference failed: {e}")))?;

        let output_view = outputs[0]
            .try_extract_array::<f32>()
            .map_err(|e| EmbedError::RequestFailed(format!("output extraction: {e}")))?;

        let shape = output_view.shape();
        let hidden_size = shape.last().copied().unwrap_or(DEFAULT_DIMS);
        let mut pooled = vec![0.0f32; hidden_size];
        let mut mask_sum = 0.0f32;

        for (tok_idx, &mask_val) in attention_mask.iter().enumerate() {
            let mask_f = mask_val as f32;
            mask_sum += mask_f;
            for dim in 0..hidden_size {
                pooled[dim] += output_view[[0, tok_idx, dim]] * mask_f;
            }
        }

        if mask_sum > 0.0 {
            for val in &mut pooled {
                *val /= mask_sum;
            }
        }

        let norm: f32 = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for val in &mut pooled {
                *val /= norm;
            }
        }

        Ok(pooled)
    }
}

#[async_trait]
impl EmbeddingService for LocalEmbeddingService {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let svc = LocalEmbeddingService {
            session: self.session.clone(),
            tokenizer: self.tokenizer.clone(),
            model_name: self.model_name.clone(),
        };
        let text = text.to_string();

        tokio::task::spawn_blocking(move || svc.embed_sync(&text))
            .await
            .map_err(|e| EmbedError::RequestFailed(format!("spawn_blocking failed: {e}")))?
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }
}

fn resolve_model_dir(model_name: &str) -> PathBuf {
    if let Ok(dir) = std::env::var("OMEGON_EMBED_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("omegon")
        .join("models")
        .join(model_name);
    config_dir
}

/// Check if local embedding models are available without loading them.
pub fn local_model_available() -> bool {
    let model_name =
        std::env::var("OMEGON_EMBED_LOCAL_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
    let dir = resolve_model_dir(&model_name);
    dir.join("model.onnx").exists() && dir.join("tokenizer.json").exists()
}

/// Returns the path where model files should be placed.
pub fn model_dir_path() -> PathBuf {
    let model_name =
        std::env::var("OMEGON_EMBED_LOCAL_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
    resolve_model_dir(&model_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_model_dir_uses_config() {
        let dir = resolve_model_dir("test-model");
        assert!(
            dir.ends_with("omegon/models/test-model")
                || dir.ends_with("omegon\\models\\test-model")
        );
    }

    #[test]
    fn local_model_not_available_when_no_files() {
        unsafe { std::env::set_var("OMEGON_EMBED_MODEL_DIR", "/nonexistent/path") };
        assert!(!local_model_available());
        unsafe { std::env::remove_var("OMEGON_EMBED_MODEL_DIR") };
    }

    #[test]
    fn load_fails_gracefully_when_no_model() {
        let tmp = tempfile::tempdir().unwrap();
        let result = LocalEmbeddingService::load(tmp.path(), "test");
        assert!(result.is_err());
        let err = match result {
            Err(e) => e.to_string(),
            Ok(_) => unreachable!(),
        };
        assert!(err.contains("model files not found"));
    }
}
