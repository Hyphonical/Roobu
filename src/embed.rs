use std::path::Path;
use std::sync::Mutex;

use image::{DynamicImage, imageops::FilterType};
use ndarray::{Array2, Array4, s};
use ort::{
	inputs,
	session::{Session, builder::AutoDevicePolicy},
	value::Tensor,
};
use tokenizers::Tokenizer;

use crate::error::RoobuError;

pub const EMBED_DIM: usize = 1024;
const SEQ_LEN: usize = 64;

pub struct Embedder {
	vision: Mutex<Session>,
	text: Mutex<Session>,
	tokenizer: Tokenizer,
}

unsafe impl Send for Embedder {}
unsafe impl Sync for Embedder {}

impl Embedder {
	pub fn new(models_dir: &Path) -> Result<Self, RoobuError> {
		let vision = Session::builder()?
			.with_auto_device(AutoDevicePolicy::MaxPerformance)
			.map_err(|e| RoobuError::Onnx(e.into()))?
			.commit_from_file(models_dir.join("vision_model_q4f16.onnx"))?;

		let text = Session::builder()?
			.with_auto_device(AutoDevicePolicy::MaxPerformance)
			.map_err(|e| RoobuError::Onnx(e.into()))?
			.commit_from_file(models_dir.join("text_model_q4f16.onnx"))?;

		tracing::debug!(
			"vision inputs:  {:?}",
			vision.inputs().iter().map(|i| i.name()).collect::<Vec<_>>()
		);
		tracing::debug!(
			"vision outputs: {:?}",
			vision
				.outputs()
				.iter()
				.map(|o| o.name())
				.collect::<Vec<_>>()
		);
		tracing::debug!(
			"text inputs:    {:?}",
			text.inputs().iter().map(|i| i.name()).collect::<Vec<_>>()
		);
		tracing::debug!(
			"text outputs:   {:?}",
			text.outputs().iter().map(|o| o.name()).collect::<Vec<_>>()
		);

		let tokenizer =
			Tokenizer::from_file(models_dir.join("tokenizer.json")).map_err(RoobuError::from)?;

		Ok(Self {
			vision: Mutex::new(vision),
			text: Mutex::new(text),
			tokenizer,
		})
	}

	pub fn preprocess(img: &DynamicImage) -> DynamicImage {
		let (w, h) = (img.width(), img.height());
		let scale = 256.0 / w.min(h) as f32;
		let new_w = (w as f32 * scale).round() as u32;
		let new_h = (h as f32 * scale).round() as u32;
		let resized = img.resize_exact(new_w, new_h, FilterType::Lanczos3);
		let x = new_w.saturating_sub(256) / 2;
		let y = new_h.saturating_sub(256) / 2;
		resized.crop_imm(x, y, 256, 256)
	}

	fn to_tensor(img: &DynamicImage) -> Array4<f32> {
		let rgb = img.to_rgb8();
		let mut arr = Array4::<f32>::zeros((1, 3, 256, 256));
		for y in 0..256usize {
			for x in 0..256usize {
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
		if images.is_empty() {
			return Err(RoobuError::EmptyBatch);
		}

		let n = images.len();
		let mut batch = Array4::<f32>::zeros((n, 3, 256, 256));
		for (i, img) in images.iter().enumerate() {
			let t = Self::to_tensor(img);
			batch
				.slice_mut(s![i, .., .., ..])
				.assign(&t.slice(s![0, .., .., ..]));
		}

		let vision_input = Tensor::from_array(batch)?;
		let mut session = self
			.vision
			.lock()
			.map_err(|_| RoobuError::Tokenizer("vision mutex poisoned".into()))?;
		let outputs = session.run(inputs!["pixel_values" => vision_input])?;
		let (shape, data) = outputs["pooler_output"].try_extract_tensor::<f32>()?;

		expect_shape(&shape, n, EMBED_DIM)?;
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

	pub fn embed_text(&self, text: &str) -> Result<[f32; EMBED_DIM], RoobuError> {
		let text = if text.trim().is_empty() {
			"unknown"
		} else {
			text
		};

		let enc = self
			.tokenizer
			.encode(text, true)
			.map_err(RoobuError::from)?;
		let mut ids: Vec<i64> = enc.get_ids().iter().map(|&x| i64::from(x)).collect();
		ids.truncate(SEQ_LEN);
		ids.resize(SEQ_LEN, 0);

		let input = Array2::from_shape_vec((1, SEQ_LEN), ids)
			.map_err(|e| RoobuError::Tokenizer(e.to_string()))?;
		let tensor = Tensor::from_array(input)?;

		let mut session = self
			.text
			.lock()
			.map_err(|_| RoobuError::Tokenizer("text mutex poisoned".into()))?;
		let outputs = session.run(inputs!["input_ids" => tensor])?;
		let (shape, data) = outputs["pooler_output"].try_extract_tensor::<f32>()?;

		expect_shape(&shape, 1, EMBED_DIM)?;
		l2_normalize(&data[..EMBED_DIM])
	}
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

pub fn cosine(a: &[f32; EMBED_DIM], b: &[f32; EMBED_DIM]) -> f32 {
	a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}
