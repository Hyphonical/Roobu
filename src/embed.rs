use std::path::Path;
use std::sync::Mutex;

use clap::ValueEnum;
use image::{DynamicImage, imageops::FilterType};
use ndarray::{Array2, Array4, s};
use ort::{
	inputs,
	session::{
		Session,
		builder::{AutoDevicePolicy, GraphOptimizationLevel},
	},
	value::Tensor,
};
use tokenizers::Tokenizer;

use crate::config;
use crate::error::RoobuError;

pub const EMBED_DIM: usize = 1024;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OnnxOptimizationIntensity {
	Safe,
	Balanced,
	Aggressive,
}

impl OnnxOptimizationIntensity {
	fn graph_level(self) -> GraphOptimizationLevel {
		match self {
			Self::Safe => GraphOptimizationLevel::Level1,
			Self::Balanced => GraphOptimizationLevel::Level2,
			Self::Aggressive => GraphOptimizationLevel::All,
		}
	}
}

#[derive(Clone, Copy, Debug)]
pub enum ModelLoad {
	TextOnly,
	VisionOnly,
	TextAndVision,
}

pub struct Embedder {
	vision: Option<Mutex<Session>>,
	text: Option<Mutex<Session>>,
	tokenizer: Option<Tokenizer>,
}

unsafe impl Send for Embedder {}
unsafe impl Sync for Embedder {}

impl Embedder {
	pub fn new(
		models_dir: &Path,
		model_load: ModelLoad,
		onnx_optimization: OnnxOptimizationIntensity,
	) -> Result<Self, RoobuError> {
		let vision_path = models_dir.join("vision_model_q4f16.onnx");
		let text_path = models_dir.join("text_model_q4f16.onnx");
		let graph_level = onnx_optimization.graph_level();

		let vision = if matches!(model_load, ModelLoad::VisionOnly | ModelLoad::TextAndVision) {
			let session = create_session(&vision_path, graph_level)?;
			log_session_io("vision", &session);
			Some(Mutex::new(session))
		} else {
			None
		};

		let text = if matches!(model_load, ModelLoad::TextOnly | ModelLoad::TextAndVision) {
			let session = create_session(&text_path, graph_level)?;
			log_session_io("text", &session);
			Some(Mutex::new(session))
		} else {
			None
		};

		let tokenizer = if matches!(model_load, ModelLoad::TextOnly | ModelLoad::TextAndVision) {
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

	pub fn preprocess(img: &DynamicImage) -> DynamicImage {
		let image_size = config::SIGLIP_IMAGE_SIZE;
		let (w, h) = (img.width(), img.height());
		let scale = image_size as f32 / w.min(h) as f32;
		let new_w = (w as f32 * scale).round() as u32;
		let new_h = (h as f32 * scale).round() as u32;
		let resized = img.resize_exact(new_w, new_h, FilterType::Lanczos3);
		let x = new_w.saturating_sub(image_size) / 2;
		let y = new_h.saturating_sub(image_size) / 2;
		resized.crop_imm(x, y, image_size, image_size)
	}

	fn to_tensor(img: &DynamicImage) -> Array4<f32> {
		let image_size = config::SIGLIP_IMAGE_SIZE as usize;
		let rgb = img.to_rgb8();
		let mut arr = Array4::<f32>::zeros((1, 3, image_size, image_size));
		for y in 0..image_size {
			for x in 0..image_size {
				let p = rgb.get_pixel(x as u32, y as u32);
				for c in 0..3 {
					arr[[0, c, y, x]] = p[c] as f32 / 255.0 * 2.0 - 1.0;
				}
			}
		}
		arr
	}

	pub fn embed_images(
		&self,
		images: &[DynamicImage],
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
		let mut batch = Array4::<f32>::zeros((n, 3, image_size, image_size));
		for (i, img) in images.iter().enumerate() {
			let t = Self::to_tensor(img);
			batch
				.slice_mut(s![i, .., .., ..])
				.assign(&t.slice(s![0, .., .., ..]));
		}

		let vision_input = Tensor::from_array(batch)?;
		let mut session = vision
			.lock()
			.map_err(|_| RoobuError::Tokenizer("vision mutex poisoned".into()))?;
		let outputs = session.run(inputs!["pixel_values" => vision_input])?;
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

	pub fn embed_image(&self, img: &DynamicImage) -> Result<[f32; EMBED_DIM], RoobuError> {
		Ok(self.embed_images(std::slice::from_ref(img))?[0])
	}

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

		let input = Array2::from_shape_vec((rows, seq_len), flat_ids)
			.map_err(|e| RoobuError::Tokenizer(e.to_string()))?;
		let tensor = Tensor::from_array(input)?;

		let mut session = text_session
			.lock()
			.map_err(|_| RoobuError::Tokenizer("text mutex poisoned".into()))?;
		let outputs = session.run(inputs!["input_ids" => tensor])?;
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

	pub fn embed_text(&self, text: &str) -> Result<[f32; EMBED_DIM], RoobuError> {
		let texts = vec![text.to_string()];
		Ok(self.embed_texts(&texts)?[0])
	}
}

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

fn log_session_io(model_name: &str, session: &Session) {
	tracing::debug!(
		"{model_name} inputs:  {:?}",
		session
			.inputs()
			.iter()
			.map(|i| i.name())
			.collect::<Vec<_>>()
	);
	tracing::debug!(
		"{model_name} outputs: {:?}",
		session
			.outputs()
			.iter()
			.map(|o| o.name())
			.collect::<Vec<_>>()
	);
}

fn create_session(model_path: &Path, level: GraphOptimizationLevel) -> Result<Session, RoobuError> {
	let mut builder = Session::builder()
		.map_err(RoobuError::Onnx)?
		.with_auto_device(AutoDevicePolicy::MaxPerformance)
		.map_err(|e| RoobuError::Onnx(e.into()))?
		.with_optimization_level(level)
		.map_err(|e| RoobuError::Onnx(e.into()))?;

	builder
		.commit_from_file(model_path)
		.map_err(RoobuError::Onnx)
}

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

pub fn blend_embeddings(
	text: &[f32; EMBED_DIM],
	image: &[f32; EMBED_DIM],
	image_weight: f32,
) -> Result<[f32; EMBED_DIM], RoobuError> {
	let text_weight = 1.0 - image_weight;
	let mut blended = [0.0f32; EMBED_DIM];
	for i in 0..EMBED_DIM {
		blended[i] = text_weight * text[i] + image_weight * image[i];
	}
	l2_normalize(&blended)
}
