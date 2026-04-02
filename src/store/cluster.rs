//! Vector fetching for clustering analysis.

use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
	Condition, Filter, ScrollPointsBuilder, VectorsSelector, point_id::PointIdOptions,
};

use super::ClusterPoint;
use super::schema;
use crate::config;
use crate::embed::EMBED_DIM;
use crate::error::RoobuError;

/// Fetch image vectors from Qdrant for clustering analysis.
///
/// Scrolls through the collection, applying an optional site filter,
/// and returns up to `max_points` vectors with their metadata.
pub async fn fetch_image_vectors(
	client: &Qdrant,
	site_filter: Option<&str>,
	page_size: u32,
	max_points: usize,
) -> Result<Vec<ClusterPoint>, RoobuError> {
	if page_size == 0 || max_points == 0 {
		return Ok(Vec::new());
	}

	let filter =
		site_filter.map(|site| Filter::must([Condition::matches("site", site.to_string())]));

	let mut offset = None;
	let mut points = Vec::new();

	while points.len() < max_points {
		let remaining = max_points - points.len();
		let limit = remaining.min(page_size as usize).min(u32::MAX as usize) as u32;

		if limit == 0 {
			break;
		}

		let mut request = ScrollPointsBuilder::new(config::QDRANT_COLLECTION)
			.limit(limit)
			.with_payload(true)
			.with_vectors(VectorsSelector {
				names: vec!["image".to_string()],
			});

		if let Some(ref f) = filter {
			request = request.filter(f.clone());
		}

		if let Some(current_offset) = offset {
			request = request.offset(current_offset);
		}

		let response = client.scroll(request).await?;
		let next_offset = response.next_page_offset;

		if response.result.is_empty() {
			break;
		}

		for point in response.result {
			let Some(id) = point
				.id
				.as_ref()
				.and_then(|id| id.point_id_options.as_ref())
			else {
				continue;
			};

			let PointIdOptions::Num(point_id) = id else {
				continue;
			};

			let (_, post_id) = schema::decode_point_id(*point_id);

			let Some(vectors) = point.vectors.as_ref() else {
				continue;
			};

			let Some(image_vec) = schema::extract_named_dense_vector(vectors, "image") else {
				continue;
			};

			if image_vec.len() != EMBED_DIM {
				continue;
			}

			let post_url = point
				.payload
				.get("post_url")
				.and_then(|value| value.as_str())
				.map_or_else(String::new, |s| s.to_owned());

			points.push(ClusterPoint {
				post_id,
				post_url,
				image_vec,
			});

			if points.len() >= max_points {
				break;
			}
		}

		offset = next_offset;
		if offset.is_none() {
			break;
		}
	}

	Ok(points)
}
