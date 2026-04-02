//! Point ID encoding and payload extraction helpers.
//!
//! Handles encoding/decoding of (site_namespace, post_id) pairs into
//! single u64 point IDs, and extracting typed values from Qdrant payloads.

use std::collections::BTreeMap;

use qdrant_client::qdrant::{vector_output, vectors_output};

use crate::config;

/// Encode a (site_namespace, post_id) pair into a single u64 point ID.
///
/// The site namespace occupies the high-order digits, ensuring unique IDs
/// across all sites. The post_id can be up to 999,999,999,999.
pub fn encode_point_id(site_ns: u64, post_id: u64) -> u64 {
	site_ns * config::POINT_ID_SITE_MULTIPLIER + post_id
}

/// Decode a u64 point ID back into (site_namespace, post_id).
pub fn decode_point_id(point_id: u64) -> (u64, u64) {
	(
		point_id / config::POINT_ID_SITE_MULTIPLIER,
		point_id % config::POINT_ID_SITE_MULTIPLIER,
	)
}

/// Distribution of indexed points across sites.
pub struct SiteDistribution {
	pub total_points: u64,
	pub per_site: BTreeMap<String, u64>,
	pub missing_site_payload: u64,
}

/// Extract a named dense vector from Qdrant's vector output.
pub fn extract_named_dense_vector(
	vectors: &qdrant_client::qdrant::VectorsOutput,
	name: &str,
) -> Option<Vec<f32>> {
	let named = match vectors.vectors_options.as_ref()? {
		vectors_output::VectorsOptions::Vectors(named) => named,
		vectors_output::VectorsOptions::Vector(_) => return None,
	};

	let vector = named.vectors.get(name)?;
	match vector.vector.as_ref()? {
		vector_output::Vector::Dense(dense) => Some(dense.data.clone()),
		vector_output::Vector::Sparse(_) | vector_output::Vector::MultiDense(_) => None,
	}
}

/// Extract a string value from a Qdrant payload field.
pub fn payload_string(
	payload: &std::collections::HashMap<String, qdrant_client::qdrant::Value>,
	key: &str,
) -> String {
	payload
		.get(key)
		.and_then(|value| match value.kind.as_ref() {
			Some(qdrant_client::qdrant::value::Kind::StringValue(v)) => Some(v.clone()),
			Some(qdrant_client::qdrant::value::Kind::IntegerValue(v)) => Some(v.to_string()),
			Some(qdrant_client::qdrant::value::Kind::DoubleValue(v)) => Some(v.to_string()),
			Some(qdrant_client::qdrant::value::Kind::BoolValue(v)) => Some(v.to_string()),
			_ => None,
		})
		.unwrap_or_default()
}

/// Extract a u32 value from a Qdrant payload field.
pub fn payload_u32(
	payload: &std::collections::HashMap<String, qdrant_client::qdrant::Value>,
	key: &str,
) -> u32 {
	payload
		.get(key)
		.and_then(|value| match value.kind.as_ref() {
			Some(qdrant_client::qdrant::value::Kind::IntegerValue(v)) => u32::try_from(*v).ok(),
			Some(qdrant_client::qdrant::value::Kind::DoubleValue(v)) => {
				if *v >= 0.0 && *v <= u32::MAX as f64 {
					Some(*v as u32)
				} else {
					None
				}
			}
			Some(qdrant_client::qdrant::value::Kind::StringValue(v)) => v.parse::<u32>().ok(),
			_ => None,
		})
		.unwrap_or_default()
}

/// Extract an i64 value from a Qdrant payload field.
pub fn payload_i64(
	payload: &std::collections::HashMap<String, qdrant_client::qdrant::Value>,
	key: &str,
) -> i64 {
	payload
		.get(key)
		.and_then(|value| match value.kind.as_ref() {
			Some(qdrant_client::qdrant::value::Kind::IntegerValue(v)) => Some(*v),
			Some(qdrant_client::qdrant::value::Kind::DoubleValue(v)) => Some(*v as i64),
			Some(qdrant_client::qdrant::value::Kind::StringValue(v)) => v.parse::<i64>().ok(),
			_ => None,
		})
		.unwrap_or_default()
}

#[cfg(test)]
mod tests {
	use super::{decode_point_id, encode_point_id};

	#[test]
	fn point_id_roundtrip_preserves_site_and_post_id() {
		let encoded = encode_point_id(2, 6_290_764);
		let (site_ns, post_id) = decode_point_id(encoded);

		assert_eq!(site_ns, 2);
		assert_eq!(post_id, 6_290_764);
	}

	#[test]
	fn different_site_namespaces_produce_unique_ids() {
		let rule34 = encode_point_id(1, 42);
		let e621 = encode_point_id(2, 42);

		assert_ne!(rule34, e621);
	}
}
