//! Image preprocessing for SigLIP embedding.
//!
//! Handles resizing, center-cropping, and tensor conversion for images
//! before they are passed through the vision model.

use image::{DynamicImage, imageops::FilterType};
use ndarray::Array4;

use crate::config;

/// Preprocess an image for SigLIP embedding.
///
/// Resizes the image so the shorter edge matches [`config::SIGLIP_IMAGE_SIZE`],
/// then center-crops to a square. Uses Lanczos3 resampling for quality.
pub fn preprocess_image(img: &DynamicImage) -> DynamicImage {
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

/// Convert a preprocessed image into a normalized NCHW tensor.
///
/// Pixel values are scaled from [0, 255] to [-1, 1] as expected by SigLIP.
pub fn image_to_tensor(img: &DynamicImage) -> Array4<f32> {
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
