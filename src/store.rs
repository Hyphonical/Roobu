//! Qdrant vector database client and schema management.
//!
//! Provides the [`Store`] struct for interacting with Qdrant, including
//! collection creation, point upsertion, semantic search, vector fetching
//! for clustering, and site distribution statistics.

use std::collections::BTreeMap;

use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
	Condition, CreateCollectionBuilder, CreateFieldIndexCollectionBuilder, Distance, FieldType,
	Filter, NamedVectors, PointStruct, QuantizationType, ScalarQuantizationBuilder,
	ScrollPointsBuilder, SearchPointsBuilder, SearchResponse, UpsertPointsBuilder,
	VectorParamsBuilder, VectorsConfigBuilder, VectorsSelector, point_id::PointIdOptions,
	vector_output, vectors_output,
};

use crate::config;
use crate::embed::EMBED_DIM;
use crate::error::RoobuError;
use crate::{ui_step, ui_success};

// ── Point ID Encoding ───────────────────────────────────────────────────────

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

// ── Data Structures ─────────────────────────────────────────────────────────

/// Represents an embedded post ready for upsertion into Qdrant.
pub struct PostEmbedding {
	pub post_id: u64,
	pub site: &'static str,
	pub site_namespace: u64,
	pub post_url: String,
	pub thumbnail_url: String,
	pub direct_image_url: String,
	pub tags: String,
	pub width: u32,
	pub height: u32,
	pub ingestion_date: i64,
	pub rating: String,
	pub image_vec: [f32; EMBED_DIM],
}

/// A single search result returned from Qdrant.
pub struct SearchResult {
	pub post_id: u64,
	pub post_url: String,
	pub thumbnail_url: String,
	pub direct_image_url: String,
	pub width: u32,
	pub height: u32,
	pub ingestion_date: i64,
	pub score: f32,
}

/// A point fetched from Qdrant for clustering, containing its embedding vector.
pub struct ClusterPoint {
	pub post_id: u64,
	pub post_url: String,
	pub image_vec: Vec<f32>,
}

/// Distribution of indexed points across sites.
pub struct SiteDistribution {
	pub total_points: u64,
	pub per_site: BTreeMap<String, u64>,
	pub missing_site_payload: u64,
}

// ── Store ───────────────────────────────────────────────────────────────────

/// Qdrant client wrapper with collection management and query helpers.
pub struct Store {
	client: Qdrant,
}

impl Store {
	/// Create a new store and ensure the collection exists.
	pub async fn new(url: &str) -> Result<Self, RoobuError> {
		let client = Qdrant::from_url(url).build().map_err(RoobuError::from)?;
		let store = Self { client };
		store.ensure_collection().await?;
		Ok(store)
	}

	/// Create the Qdrant collection and field indexes if they don't exist.
	async fn ensure_collection(&self) -> Result<(), RoobuError> {
		if self
			.client
			.collection_exists(config::QDRANT_COLLECTION)
			.await?
		{
			tracing::debug!("collection '{}' exists", config::QDRANT_COLLECTION);
			return Ok(());
		}

		ui_step!(
			"Creating Qdrant collection '{}'…",
			config::QDRANT_COLLECTION
		);

		let mut vectors = VectorsConfigBuilder::default();
		vectors.add_named_vector_params(
			"image",
			VectorParamsBuilder::new(EMBED_DIM as u64, Distance::Cosine)
				.on_disk(true)
				.quantization_config(
					ScalarQuantizationBuilder::default()
						.r#type(QuantizationType::Int8.into())
						.quantile(0.99)
						.always_ram(false),
				),
		);

		self.client
			.create_collection(
				CreateCollectionBuilder::new(config::QDRANT_COLLECTION).vectors_config(vectors),
			)
			.await?;

		// Create field indexes for efficient filtering.
		for (field, field_type) in [
			("post_id", FieldType::Integer),
			("site", FieldType::Keyword),
			("ingestion_date", FieldType::Integer),
		] {
			self.client
				.create_field_index(
					CreateFieldIndexCollectionBuilder::new(
						config::QDRANT_COLLECTION,
						field,
						field_type,
					)
					.wait(true),
				)
				.await?;
		}

		ui_success!("Collection ready");
		Ok(())
	}

	/// Upsert a batch of embedded posts into Qdrant.
	pub async fn upsert(&self, embeddings: Vec<PostEmbedding>) -> Result<(), RoobuError> {
		if embeddings.is_empty() {
			return Ok(());
		}

		let points: Vec<PointStruct> = embeddings
			.into_iter()
			.map(|e| {
				let point_id = encode_point_id(e.site_namespace, e.post_id);
				let vectors = NamedVectors::default().add_vector("image", e.image_vec.to_vec());

				let payload: serde_json::Map<String, serde_json::Value> = [
					("post_id".to_string(), serde_json::json!(e.post_id as i64)),
					("site".to_string(), serde_json::json!(e.site)),
					("post_url".to_string(), serde_json::json!(e.post_url)),
					(
						"thumbnail_url".to_string(),
						serde_json::json!(e.thumbnail_url),
					),
					(
						"direct_image_url".to_string(),
						serde_json::json!(e.direct_image_url),
					),
					("tags".to_string(), serde_json::json!(e.tags)),
					("width".to_string(), serde_json::json!(e.width as i64)),
					("height".to_string(), serde_json::json!(e.height as i64)),
					(
						"ingestion_date".to_string(),
						serde_json::json!(e.ingestion_date),
					),
					("rating".to_string(), serde_json::json!(e.rating)),
				]
				.into_iter()
				.collect();

				PointStruct::new(point_id, vectors, payload)
			})
			.collect();

		self.client
			.upsert_points(UpsertPointsBuilder::new(config::QDRANT_COLLECTION, points).wait(true))
			.await?;

		Ok(())
	}

	/// Search for similar images using a query vector.
	///
	/// Fetches `limit * SEARCH_FETCH_LIMIT_MULTIPLIER` candidates to account
	/// for post-filtering when a site filter is applied, then truncates to
	/// the requested limit.
	pub async fn search(
		&self,
		query_vec: Vec<f32>,
		limit: u64,
		site_filter: Option<&str>,
	) -> Result<Vec<SearchResult>, RoobuError> {
		let fetch_limit = limit.saturating_mul(config::SEARCH_FETCH_LIMIT_MULTIPLIER);

		let filter =
			site_filter.map(|site| Filter::must([Condition::matches("site", site.to_string())]));

		let mut search =
			SearchPointsBuilder::new(config::QDRANT_COLLECTION, query_vec, fetch_limit)
				.vector_name("image")
				.with_payload(true);

		if let Some(ref f) = filter {
			search = search.filter(f.clone());
		}

		let response: SearchResponse = self
			.client
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

				let (_, raw_post_id) = decode_point_id(*point_id);
				Some(SearchResult {
					post_id: raw_post_id,
					post_url: payload_string(&point.payload, "post_url"),
					thumbnail_url: payload_string(&point.payload, "thumbnail_url"),
					direct_image_url: payload_string(&point.payload, "direct_image_url"),
					width: payload_u32(&point.payload, "width"),
					height: payload_u32(&point.payload, "height"),
					ingestion_date: payload_i64(&point.payload, "ingestion_date"),
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

	/// Fetch image vectors from Qdrant for clustering analysis.
	///
	/// Scrolls through the collection, applying an optional site filter,
	/// and returns up to `max_points` vectors with their metadata.
	pub async fn fetch_image_vectors_for_clustering(
		&self,
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

			let response = self.client.scroll(request).await?;
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

				let (_, post_id) = decode_point_id(*point_id);

				let Some(vectors) = point.vectors.as_ref() else {
					continue;
				};

				let Some(image_vec) = extract_named_dense_vector(vectors, "image") else {
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

	/// Compute the distribution of indexed points across all sites.
	pub async fn fetch_site_counts(&self, page_size: u32) -> Result<SiteDistribution, RoobuError> {
		if page_size == 0 {
			return Ok(SiteDistribution {
				total_points: 0,
				per_site: BTreeMap::new(),
				missing_site_payload: 0,
			});
		}

		let mut offset = None;
		let mut total_points = 0u64;
		let mut missing_site_payload = 0u64;
		let mut per_site: BTreeMap<String, u64> = BTreeMap::new();

		loop {
			let mut request = ScrollPointsBuilder::new(config::QDRANT_COLLECTION)
				.limit(page_size)
				.with_payload(true);

			if let Some(current_offset) = offset {
				request = request.offset(current_offset);
			}

			let response = self.client.scroll(request).await?;
			let next_offset = response.next_page_offset;

			if response.result.is_empty() {
				break;
			}

			for point in response.result {
				total_points = total_points.saturating_add(1);

				match point.payload.get("site").and_then(|value| value.as_str()) {
					Some(site) if !site.is_empty() => {
						*per_site.entry(site.to_string()).or_insert(0) += 1;
					}
					_ => {
						missing_site_payload = missing_site_payload.saturating_add(1);
					}
				}
			}

			offset = next_offset;
			if offset.is_none() {
				break;
			}
		}

		Ok(SiteDistribution {
			total_points,
			per_site,
			missing_site_payload,
		})
	}
}

// ── Payload Helpers ─────────────────────────────────────────────────────────

/// Extract a named dense vector from Qdrant's vector output.
fn extract_named_dense_vector(
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
fn payload_string(
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
fn payload_u32(
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
fn payload_i64(
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
