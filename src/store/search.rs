//! Semantic search queries against Qdrant.

use qdrant_client::Qdrant;
use qdrant_client::qdrant::{Condition, Filter, SearchPointsBuilder, SearchResponse};

use super::SearchResult;
use super::schema;
use crate::config;
use crate::error::RoobuError;

/// Search for similar images using a query vector.
///
/// Fetches `limit * SEARCH_FETCH_LIMIT_MULTIPLIER` candidates to account
/// for post-filtering when a site filter is applied, then truncates to
/// the requested limit.
pub async fn search(
	client: &Qdrant,
	query_vec: Vec<f32>,
	limit: u64,
	site_filter: Option<&str>,
) -> Result<Vec<SearchResult>, RoobuError> {
	let fetch_limit = limit.saturating_mul(config::SEARCH_FETCH_LIMIT_MULTIPLIER);

	let filter =
		site_filter.map(|site| Filter::must([Condition::matches("site", site.to_string())]));

	let mut search = SearchPointsBuilder::new(config::QDRANT_COLLECTION, query_vec, fetch_limit)
		.vector_name("image")
		.with_payload(true);

	if let Some(ref f) = filter {
		search = search.filter(f.clone());
	}

	let response: SearchResponse = client
		.search_points(search)
		.await
		.map_err(RoobuError::from)?;

	let mut results: Vec<SearchResult> = response
		.result
		.into_iter()
		.filter_map(|point| {
			let id = point
				.id
				.as_ref()
				.and_then(|pid| pid.point_id_options.as_ref())?;
			let qdrant_client::qdrant::point_id::PointIdOptions::Num(point_id) = id else {
				return None;
			};

			let (_, raw_post_id) = schema::decode_point_id(*point_id);
			Some(SearchResult {
				post_id: raw_post_id,
				post_url: schema::payload_string(&point.payload, "post_url"),
				thumbnail_url: schema::payload_string(&point.payload, "thumbnail_url"),
				direct_image_url: schema::payload_string(&point.payload, "direct_image_url"),
				width: schema::payload_u32(&point.payload, "width"),
				height: schema::payload_u32(&point.payload, "height"),
				ingestion_date: schema::payload_i64(&point.payload, "ingestion_date"),
				score: point.score,
			})
		})
		.collect();

	// Fall back to thumbnail URL if direct image URL is missing.
	for result in &mut results {
		if result.direct_image_url.is_empty() {
			result.direct_image_url = result.thumbnail_url.clone();
		}
	}

	results.sort_by(|a, b| {
		b.score
			.partial_cmp(&a.score)
			.unwrap_or(std::cmp::Ordering::Equal)
	});
	results.truncate(limit as usize);
	Ok(results)
}
