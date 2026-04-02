//! ONNX-based embedding using SigLIP vision and text models.
//!
//! Provides the [`Embedder`] struct which loads quantized ONNX models and
//! produces 1536-dimensional embeddings for images and text. Supports batch
//! processing, L2 normalization, and hybrid text+image query blending.

mod blend;
mod model;
pub mod preprocess;

pub use blend::blend_embeddings;
pub use model::{Embedder, ModelLoad, OnnxOptimizationIntensity};

/// The dimensionality of SigLIP embeddings produced by the loaded models.
pub const EMBED_DIM: usize = 1536;
