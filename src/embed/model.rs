//! ONNX model session creation and management.
//!
//! Handles loading vision and text models with optimization level configuration
//! and fallback logic for the text model.

use std::path::Path;
use std::sync::Mutex;

use clap::ValueEnum;
use ort::session::{
	Session,
	builder::{AutoDevicePolicy, GraphOptimizationLevel},
};
use tokenizers::Tokenizer;

use crate::config;
use crate::embed::EMBED_DIM;
use crate::error::RoobuError;

/// ONNX graph optimization intensity.
///
/// Higher levels may improve inference speed but can cause compatibility
/// issues with certain model exports. The text model uses a fallback strategy
/// that tries lower optimization levels if the requested one fails.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OnnxOptimizationIntensity {
	/// Minimal optimization — maximum compatibility.
	Safe,
	/// Moderate optimization — good balance of speed and compatibility.
	Balanced,
	/// Full optimization — fastest but may fail with some models.
	Aggressive,
}

impl OnnxOptimizationIntensity {
	/// Map the intensity to the corresponding ONNX Runtime graph optimization level.
	fn graph_level(self) -> GraphOptimizationLevel {
		match self {
			Self::Safe => GraphOptimizationLevel::Level1,
			Self::Balanced => GraphOptimizationLevel::Level2,
			Self::Aggressive => GraphOptimizationLevel::All,
		}
	}
}

/// Specifies which model components to load.
///
/// Loading only the components needed for a given operation reduces
/// memory usage and startup time.
#[derive(Clone, Copy, Debug)]
pub enum ModelLoad {
	/// Load only the text model and tokenizer (text-only search).
	TextOnly,
	/// Load only the vision model (image-only search or ingest).
	VisionOnly,
	/// Load both text and vision models (hybrid search).
	TextAndVision,
}

/// SigLIP embedder for producing vector representations of images and text.
///
/// The embedder wraps ONNX Runtime sessions behind [`Mutex`] guards to enable
/// safe concurrent access from multiple threads. The underlying `ort::Session`
/// is `Send + Sync` by construction in ort 2.x, but the [`Mutex`] wrapper
/// provides an additional safety guarantee at the type level.
pub struct Embedder {
	/// Vision model session, guarded for thread-safe access.
	vision: Option<Mutex<Session>>,
	/// Text model session, guarded for thread-safe access.
	text: Option<Mutex<Session>>,
	/// Tokenizer for converting text to input IDs.
	tokenizer: Option<Tokenizer>,
}

impl Embedder {
	/// Create a new embedder, loading the specified model components.
	///
	/// # Arguments
	/// * `models_dir` — Directory containing the ONNX model files and tokenizer.
	/// * `model_load` — Which components to load (text, vision, or both).
	/// * `onnx_optimization` — Graph optimization intensity level.
	///
	/// # Errors
	/// Returns an error if model files are missing, malformed, or incompatible
	/// with the requested optimization level.
	pub fn new(
		models_dir: &Path,
		model_load: ModelLoad,
		onnx_optimization: OnnxOptimizationIntensity,
	) -> Result<Self, RoobuError> {
		let vision_path = models_dir.join("vision_model_q4f16.onnx");
		let text_path = models_dir.join("text_model_q4f16.onnx");
		let graph_level = onnx_optimization.graph_level();

		let load_vision = matches!(model_load, ModelLoad::VisionOnly | ModelLoad::TextAndVision);
		let load_text = matches!(model_load, ModelLoad::TextOnly | ModelLoad::TextAndVision);

		let vision = if load_vision {
			let session = create_session(&vision_path, graph_level, true)?;
			log_session_io("vision", &session);
			Some(Mutex::new(session))
		} else {
			None
		};

		let text = if load_text {
			let session = create_text_session_with_fallback(&text_path, onnx_optimization)?;
			log_session_io("text", &session);
			Some(Mutex::new(session))
		} else {
			None
		};

		let tokenizer = if load_text {
			Some(
				Tokenizer::from_file(models_dir.join("tokenizer.json"))
					.map_err(RoobuError::from)?,
			)
		} else {
			None
		};

		Ok(Self {
			vision,
			text,
			tokenizer,
		})
	}

	/// Preprocess an image for SigLIP embedding.
	///
	/// Resizes the image so the shorter edge matches [`config::SIGLIP_IMAGE_SIZE`],
	/// then center-crops to a square. Uses Lanczos3 resampling for quality.
	pub fn preprocess(img: &image::DynamicImage) -> image::DynamicImage {
		super::preprocess::preprocess_image(img)
	}

	/// Embed a batch of images into L2-normalized vectors.
	///
	/// Images are preprocessed, batched into a single NCHW tensor, and passed
	/// through the vision model. The resulting embeddings are L2-normalized
	/// for cosine similarity search.
	///
	/// # Errors
	/// Returns an error if the vision model is not loaded, the batch is empty,
	/// or the ONNX inference fails.
	pub fn embed_images(
		&self,
		images: &[image::DynamicImage],
	) -> Result<Vec<[f32; EMBED_DIM]>, RoobuError> {
		let vision = self
			.vision
			.as_ref()
			.ok_or(RoobuError::ModelNotLoaded("vision model"))?;

		if images.is_empty() {
			return Err(RoobuError::EmptyBatch);
		}

		let n = images.len();
		let image_size = config::SIGLIP_IMAGE_SIZE as usize;
		let mut batch = ndarray::Array4::<f32>::zeros((n, 3, image_size, image_size));
		for (i, img) in images.iter().enumerate() {
			let t = super::preprocess::image_to_tensor(img);
			batch
				.slice_mut(ndarray::s![i, .., .., ..])
				.assign(&t.slice(ndarray::s![0, .., .., ..]));
		}

		let vision_input = ort::value::Tensor::from_array(batch)?;
		let mut session = vision
			.lock()
			.map_err(|_| RoobuError::Tokenizer("vision mutex poisoned".into()))?;
		let outputs = session.run(ort::inputs!["pixel_values" => vision_input])?;
		let (shape, data) = outputs["pooler_output"].try_extract_tensor::<f32>()?;

		expect_shape(shape, n, EMBED_DIM)?;
		let cols = shape[1] as usize;

		(0..n)
			.map(|i| {
				let start = i * cols;
				l2_normalize(&data[start..start + cols])
			})
			.collect()
	}

	/// Embed a single image into an L2-normalized vector.
	pub fn embed_image(&self, img: &image::DynamicImage) -> Result<[f32; EMBED_DIM], RoobuError> {
		Ok(self.embed_images(std::slice::from_ref(img))?[0])
	}

	/// Embed a batch of text strings into L2-normalized vectors.
	///
	/// Each text is tokenized, padded/truncated to [`config::SIGLIP_TEXT_SEQ_LEN`],
	/// and passed through the text model. The resulting embeddings are
	/// L2-normalized for cosine similarity search.
	///
	/// # Errors
	/// Returns an error if the text model or tokenizer is not loaded, the batch
	/// is empty, or the ONNX inference fails.
	pub fn embed_texts(&self, texts: &[String]) -> Result<Vec<[f32; EMBED_DIM]>, RoobuError> {
		let tokenizer = self
			.tokenizer
			.as_ref()
			.ok_or(RoobuError::ModelNotLoaded("text tokenizer"))?;
		let text_session = self
			.text
			.as_ref()
			.ok_or(RoobuError::ModelNotLoaded("text model"))?;

		if texts.is_empty() {
			return Err(RoobuError::EmptyBatch);
		}

		let rows = texts.len();
		let seq_len = config::SIGLIP_TEXT_SEQ_LEN;
		let mut flat_ids: Vec<i64> = Vec::with_capacity(rows * seq_len);

		for text in texts {
			flat_ids.extend(encode_text_ids(tokenizer, text)?);
		}

		let input = ndarray::Array2::from_shape_vec((rows, seq_len), flat_ids)
			.map_err(|e| RoobuError::Tokenizer(e.to_string()))?;
		let tensor = ort::value::Tensor::from_array(input)?;

		let mut session = text_session
			.lock()
			.map_err(|_| RoobuError::Tokenizer("text mutex poisoned".into()))?;
		let outputs = session.run(ort::inputs!["input_ids" => tensor])?;
		let (shape, data) = outputs["pooler_output"].try_extract_tensor::<f32>()?;

		expect_shape(shape, rows, EMBED_DIM)?;
		let cols = shape[1] as usize;

		(0..rows)
			.map(|i| {
				let start = i * cols;
				l2_normalize(&data[start..start + cols])
			})
			.collect()
	}

	/// Embed a single text string into an L2-normalized vector.
	pub fn embed_text(&self, text: &str) -> Result<[f32; EMBED_DIM], RoobuError> {
		let texts = vec![text.to_string()];
		Ok(self.embed_texts(&texts)?[0])
	}
}

// ── Helper Functions ────────────────────────────────────────────────────────

/// Encode a text string into a fixed-length sequence of token IDs.
///
/// Empty or whitespace-only strings are replaced with "unknown" to avoid
/// producing degenerate embeddings. The output is truncated or zero-padded
/// to [`config::SIGLIP_TEXT_SEQ_LEN`].
fn encode_text_ids(tokenizer: &Tokenizer, text: &str) -> Result<Vec<i64>, RoobuError> {
	let normalized = if text.trim().is_empty() {
		"unknown"
	} else {
		text
	};

	let enc = tokenizer
		.encode(normalized, true)
		.map_err(RoobuError::from)?;
	let mut ids: Vec<i64> = enc.get_ids().iter().map(|&x| i64::from(x)).collect();
	ids.truncate(config::SIGLIP_TEXT_SEQ_LEN);
	ids.resize(config::SIGLIP_TEXT_SEQ_LEN, 0);
	Ok(ids)
}

/// Log the input and output tensor names/types of a loaded ONNX session.
fn log_session_io(model_name: &str, session: &Session) {
	tracing::debug!(
		"{model_name} inputs:  {:?}",
		session
			.inputs()
			.iter()
			.map(|i| format!("{}: {}", i.name(), i.dtype()))
			.collect::<Vec<_>>()
	);
	tracing::debug!(
		"{model_name} outputs: {:?}",
		session
			.outputs()
			.iter()
			.map(|o| format!("{}: {}", o.name(), o.dtype()))
			.collect::<Vec<_>>()
	);
}

/// Create an ONNX session with the specified optimization level.
fn create_session(
	model_path: &Path,
	level: GraphOptimizationLevel,
	use_auto_device: bool,
) -> Result<Session, RoobuError> {
	let mut builder = Session::builder().map_err(RoobuError::Onnx)?;

	if use_auto_device {
		builder = builder
			.with_auto_device(AutoDevicePolicy::MaxPerformance)
			.map_err(|e| RoobuError::Onnx(e.into()))?;
	}

	builder = builder
		.with_optimization_level(level)
		.map_err(|e| RoobuError::Onnx(e.into()))?;

	builder
		.commit_from_file(model_path)
		.map_err(RoobuError::Onnx)
}

/// Optimization levels to try for the text model, in fallback order.
///
/// The text model may not support all optimization levels depending on how
/// it was exported. This function returns a list of levels to try, starting
/// with the requested intensity and falling back to lower levels.
fn text_fallback_levels(intensity: OnnxOptimizationIntensity) -> &'static [GraphOptimizationLevel] {
	match intensity {
		OnnxOptimizationIntensity::Aggressive => &[
			GraphOptimizationLevel::All,
			GraphOptimizationLevel::Level2,
			GraphOptimizationLevel::Level1,
		],
		OnnxOptimizationIntensity::Balanced => &[
			GraphOptimizationLevel::Level2,
			GraphOptimizationLevel::Level1,
		],
		OnnxOptimizationIntensity::Safe => &[GraphOptimizationLevel::Level1],
	}
}

/// Create a text model session with fallback to lower optimization levels.
///
/// If the requested optimization level fails, progressively lower levels are
/// tried until one succeeds. A warning is logged when a fallback occurs.
fn create_text_session_with_fallback(
	model_path: &Path,
	intensity: OnnxOptimizationIntensity,
) -> Result<Session, RoobuError> {
	let levels = text_fallback_levels(intensity);
	let mut last_error: Option<RoobuError> = None;

	for (idx, level) in levels.iter().copied().enumerate() {
		match create_session(model_path, level, false) {
			Ok(session) => {
				if idx > 0 {
					tracing::warn!(
						requested = ?intensity,
						applied = ?level,
						"text model required lower ONNX optimization level"
					);
				}
				return Ok(session);
			}
			Err(error) => {
				tracing::warn!(attempt = ?level, error = %error, "text model session initialization failed");
				last_error = Some(error);
			}
		}
	}

	Err(last_error.unwrap_or(RoobuError::ModelNotLoaded("text model")))
}

/// Validate that a tensor has the expected 2D shape [rows, cols].
fn expect_shape(shape: &[i64], rows: usize, cols: usize) -> Result<(), RoobuError> {
	if shape.len() != 2 {
		return Err(RoobuError::DimensionMismatch {
			expected: 2,
			actual: shape.len(),
		});
	}
	if shape[0] as usize != rows {
		return Err(RoobuError::DimensionMismatch {
			expected: rows,
			actual: shape[0] as usize,
		});
	}
	if shape[1] as usize != cols {
		return Err(RoobuError::DimensionMismatch {
			expected: cols,
			actual: shape[1] as usize,
		});
	}
	Ok(())
}

/// L2-normalize a slice of f32 values into a fixed-size array.
///
/// If the norm is near zero, the original values are copied unchanged to
/// avoid division by zero.
fn l2_normalize(slice: &[f32]) -> Result<[f32; EMBED_DIM], RoobuError> {
	if slice.len() != EMBED_DIM {
		return Err(RoobuError::DimensionMismatch {
			expected: EMBED_DIM,
			actual: slice.len(),
		});
	}
	let norm: f32 = slice.iter().map(|x| x * x).sum::<f32>().sqrt();
	let mut out = [0.0f32; EMBED_DIM];
	if norm > 1e-9 {
		for (o, &v) in out.iter_mut().zip(slice) {
			*o = v / norm;
		}
	} else {
		out.copy_from_slice(slice);
	}
	Ok(out)
}
