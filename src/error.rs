use thiserror::Error;

#[derive(Debug, Error)]
pub enum RoobuError {
	#[error("ONNX runtime error: {0}")]
	Onnx(#[from] ort::Error),

	#[error("tokenizer error: {0}")]
	Tokenizer(String),

	#[error("image error: {0}")]
	Image(#[from] image::ImageError),

	#[error("I/O error: {0}")]
	Io(#[from] std::io::Error),

	#[error("HTTP error: {0}")]
	Http(#[from] reqwest::Error),

	#[error("Qdrant error: {0}")]
	Qdrant(#[from] qdrant_client::QdrantError),

	#[error("API error: {0}")]
	Api(String),

	#[error("dimension mismatch: expected {expected}, got {actual}")]
	DimensionMismatch { expected: usize, actual: usize },

	#[error("empty batch — all images failed validation")]
	EmptyBatch,
}

impl From<tokenizers::Error> for RoobuError {
	fn from(e: tokenizers::Error) -> Self {
		Self::Tokenizer(e.to_string())
	}
}
