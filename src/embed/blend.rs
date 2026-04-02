//! Embedding blending utilities.
//!
//! Provides weighted combination of text and image embeddings for hybrid search.

use crate::embed::EMBED_DIM;
use crate::error::RoobuError;

/// Blend text and image embeddings with a weighted combination.
///
/// The result is L2-normalized so it can be used directly for cosine
/// similarity search. An `image_weight` of 0.0 returns the text embedding,
/// 1.0 returns the image embedding, and values in between produce a hybrid.
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
