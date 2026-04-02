//! Error types for the Roobu application.
//!
//! Defines [`RoobuError`], a comprehensive error enum covering all failure
//! modes across ONNX inference, HTTP requests, Qdrant operations, image
//! processing, and data validation.

use thiserror::Error;

/// The top-level error type for all Roobu operations.
///
/// Each variant represents a distinct failure domain, making it easy for
/// callers to match on specific error kinds when needed.
#[derive(Debug, Error)]
pub enum RoobuError {
	/// ONNX Runtime inference or session creation failed.
	#[error("ONNX runtime error: {0}")]
	Onnx(#[from] ort::Error),

	/// Tokenizer encoding failed.
	#[error("tokenizer error: {0}")]
	Tokenizer(String),

	/// Image loading or decoding failed.
	#[error("image error: {0}")]
	Image(#[from] image::ImageError),

	/// File system or I/O operation failed.
	#[error("I/O error: {0}")]
	Io(#[from] std::io::Error),

	/// HTTP request failed (connection, timeout, or non-retryable status).
	#[error("HTTP error: {0}")]
	Http(#[from] reqwest::Error),

	/// Qdrant database operation failed.
	#[error("Qdrant error: {0}")]
	Qdrant(Box<qdrant_client::QdrantError>),

	/// A site API returned an unexpected response or error message.
	#[error("API error: {0}")]
	Api(String),

	/// Tensor shape did not match the expected dimensions.
	#[error("dimension mismatch: expected {expected}, got {actual}")]
	DimensionMismatch {
		/// The expected dimension count or size.
		expected: usize,
		/// The actual dimension count or size observed.
		actual: usize,
	},

	/// A required model component (vision, text, or tokenizer) was not loaded.
	#[error("required model component not loaded: {0}")]
	ModelNotLoaded(&'static str),

	/// An embedding batch contained no valid items after filtering.
	#[error("empty batch — all images failed validation")]
	EmptyBatch,
}

impl From<tokenizers::Error> for RoobuError {
	fn from(e: tokenizers::Error) -> Self {
		Self::Tokenizer(e.to_string())
	}
}

impl From<qdrant_client::QdrantError> for RoobuError {
	fn from(e: qdrant_client::QdrantError) -> Self {
		Self::Qdrant(Box::new(e))
	}
}
