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
///
/// `site_filter` is a slice of site names to restrict results to. An empty
/// slice means no filtering (all sites). A single entry behaves like the
/// previous single-site filter. Multiple entries are combined with OR.
pub async fn search(
	client: &Qdrant,
	query_vec: Vec<f32>,
	limit: u64,
	site_filter: &[&str],
) -> Result<Vec<SearchResult>, RoobuError> {
	let fetch_limit = limit.saturating_mul(config::SEARCH_FETCH_LIMIT_MULTIPLIER);

	let filter = if site_filter.is_empty() {
		None
	} else if site_filter.len() == 1 {
		Some(Filter::must([Condition::matches(
			"site",
			site_filter[0].to_string(),
		)]))
	} else {
		// Multiple sites: must match any one of them (OR semantics).
		let conditions: Vec<Condition> = site_filter
			.iter()
			.map(|s| Condition::matches("site", s.to_string()))
			.collect();
		Some(Filter::should(conditions))
	};

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
				site: schema::payload_string(&point.payload, "site"),
				post_url: schema::payload_string(&point.payload, "post_url"),
				thumbnail_url: schema::payload_string(&point.payload, "thumbnail_url"),
				direct_image_url: schema::payload_string(&point.payload, "direct_image_url"),
				width: schema::payload_u32(&point.payload, "width"),
				height: schema::payload_u32(&point.payload, "height"),
				ingestion_date: schema::payload_i64(&point.payload, "ingestion_date"),
				score: point.score,
				tags: schema::payload_string(&point.payload, "tags"),
				rating: schema::payload_string(&point.payload, "rating"),
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

/// Convert a Qdrant retrieved point into a SearchResult.
fn point_to_result(point: &qdrant_client::qdrant::RetrievedPoint) -> Option<SearchResult> {
	let id = point
		.id
		.as_ref()
		.and_then(|pid| pid.point_id_options.as_ref())?;
	let qdrant_client::qdrant::point_id::PointIdOptions::Num(point_id) = id else {
		return None;
	};

	let (_, raw_post_id) = schema::decode_point_id(*point_id);
	let mut result = SearchResult {
		post_id: raw_post_id,
		site: schema::payload_string(&point.payload, "site"),
		post_url: schema::payload_string(&point.payload, "post_url"),
		thumbnail_url: schema::payload_string(&point.payload, "thumbnail_url"),
		direct_image_url: schema::payload_string(&point.payload, "direct_image_url"),
		width: schema::payload_u32(&point.payload, "width"),
		height: schema::payload_u32(&point.payload, "height"),
		ingestion_date: schema::payload_i64(&point.payload, "ingestion_date"),
		score: 0.0,
		tags: schema::payload_string(&point.payload, "tags"),
		rating: schema::payload_string(&point.payload, "rating"),
	};

	if result.direct_image_url.is_empty() {
		result.direct_image_url = result.thumbnail_url.clone();
	}

	Some(result)
}

/// Fetch a single post by site name and post ID.
pub async fn fetch_post(
	client: &Qdrant,
	site: &str,
	post_id: u64,
) -> Result<Option<SearchResult>, RoobuError> {
	use qdrant_client::qdrant::ScrollPointsBuilder;

	let filter = Filter::must([
		Condition::matches("site", site.to_string()),
		Condition::matches("post_id", post_id as i64),
	]);

	let response = client
		.scroll(
			ScrollPointsBuilder::new(config::QDRANT_COLLECTION)
				.limit(1)
				.filter(filter)
				.with_payload(true),
		)
		.await
		.map_err(RoobuError::from)?;

	if response.result.is_empty() {
		return Ok(None);
	}

	let point = &response.result[0];
	let result = point_to_result(point);
	Ok(result)
}

/// Find posts similar to a given post by using its embedding vector.
pub async fn search_similar(
	client: &Qdrant,
	site: &str,
	post_id: u64,
	limit: u64,
) -> Result<Vec<SearchResult>, RoobuError> {
	use qdrant_client::qdrant::{GetPointsBuilder, ScrollPointsBuilder};

	// First, fetch the target post's vector.
	let point_id = {
		let filter = Filter::must([
			Condition::matches("site", site.to_string()),
			Condition::matches("post_id", post_id as i64),
		]);
		let response = client
			.scroll(
				ScrollPointsBuilder::new(config::QDRANT_COLLECTION)
					.limit(1)
					.filter(filter)
					.with_payload(true),
			)
			.await
			.map_err(RoobuError::from)?;

		if response.result.is_empty() {
			return Err(RoobuError::Api(format!("Post {site}/{post_id} not found")));
		}

		response.result[0]
			.id
			.clone()
			.ok_or_else(|| RoobuError::Api("Point has no ID".to_string()))?
	};

	// Fetch the vector for this point.
	let vectors_response = client
		.get_points(
			GetPointsBuilder::new(config::QDRANT_COLLECTION, vec![point_id.clone()])
				.with_vectors(true),
		)
		.await
		.map_err(RoobuError::from)?;

	let query_vec = vectors_response
		.result
		.into_iter()
		.next()
		.and_then(|p| {
			super::schema::extract_named_dense_vector(&p.vectors.unwrap_or_default(), "image")
		})
		.ok_or_else(|| RoobuError::Api(format!("Post {site}/{post_id} has no image vector")))?;

	// Now search for similar posts, excluding the target itself.
	let fetch_limit = limit.saturating_mul(config::SEARCH_FETCH_LIMIT_MULTIPLIER);
	let filter = Filter::must([Condition::matches("site", site.to_string())]);

	let search = SearchPointsBuilder::new(config::QDRANT_COLLECTION, query_vec, fetch_limit)
		.vector_name("image")
		.with_payload(true)
		.filter(filter);

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
			// Skip the target post itself.
			if raw_post_id == post_id {
				return None;
			}

			Some(SearchResult {
				post_id: raw_post_id,
				site: schema::payload_string(&point.payload, "site"),
				post_url: schema::payload_string(&point.payload, "post_url"),
				thumbnail_url: schema::payload_string(&point.payload, "thumbnail_url"),
				direct_image_url: schema::payload_string(&point.payload, "direct_image_url"),
				width: schema::payload_u32(&point.payload, "width"),
				height: schema::payload_u32(&point.payload, "height"),
				ingestion_date: schema::payload_i64(&point.payload, "ingestion_date"),
				score: point.score,
				tags: schema::payload_string(&point.payload, "tags"),
				rating: schema::payload_string(&point.payload, "rating"),
			})
		})
		.collect();

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

/// Fetch the most recently ingested posts.
///
/// Uses Qdrant's `order_by` feature to sort by `ingestion_date` descending,
/// then paginates with an offset. Returns a page with a `has_more` flag.
pub async fn fetch_recent(
	client: &Qdrant,
	limit: u64,
	offset: u64,
	site_filter: &[&str],
) -> Result<super::RecentPage, RoobuError> {
	use qdrant_client::qdrant::{Direction, OrderBy, ScrollPointsBuilder};

	let filter = if site_filter.is_empty() {
		None
	} else if site_filter.len() == 1 {
		Some(Filter::must([Condition::matches(
			"site",
			site_filter[0].to_string(),
		)]))
	} else {
		let conditions: Vec<Condition> = site_filter
			.iter()
			.map(|s| Condition::matches("site", s.to_string()))
			.collect();
		Some(Filter::should(conditions))
	};

	// Order by ingestion_date descending.
	// Omit start_from to begin from the highest value (most recent).
	let order_by = OrderBy {
		key: "ingestion_date".to_string(),
		direction: Some(Direction::Desc as i32),
		start_from: None,
	};

	// Fetch limit+1 to detect if there are more results.
	let fetch_limit = limit + 1;
	let mut request = ScrollPointsBuilder::new(config::QDRANT_COLLECTION)
		.limit(fetch_limit as u32)
		.with_payload(true)
		.order_by(order_by);

	if let Some(ref f) = filter {
		request = request.filter(f.clone());
	}

	// Scroll with offset-based pagination.
	let mut all_results: Vec<SearchResult> = Vec::new();
	let mut current_offset: Option<qdrant_client::qdrant::PointId> = None;
	let mut skipped = 0u64;

	loop {
		let mut req = request.clone();
		if let Some(ref o) = current_offset {
			req = req.offset(o.clone());
		}

		let response = client.scroll(req).await.map_err(RoobuError::from)?;

		for point in response.result {
			if skipped < offset {
				skipped += 1;
				continue;
			}
			if let Some(r) = point_to_result(&point) {
				all_results.push(r);
			}
			if all_results.len() >= fetch_limit as usize {
				break;
			}
		}

		if all_results.len() >= fetch_limit as usize {
			break;
		}

		current_offset = response.next_page_offset;
		if current_offset.is_none() {
			break;
		}
	}

	let has_more = all_results.len() > limit as usize;
	all_results.truncate(limit as usize);

	Ok(super::RecentPage {
		posts: all_results,
		has_more,
	})
}
