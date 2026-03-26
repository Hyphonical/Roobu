use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
	Condition, CreateCollectionBuilder, CreateFieldIndexCollectionBuilder, Distance, FieldType,
	Filter, NamedVectors, PointStruct, QuantizationType, ScalarQuantizationBuilder,
	SearchPointsBuilder, SearchResponse, UpsertPointsBuilder, VectorParamsBuilder,
	VectorsConfigBuilder,
};

use crate::embed::EMBED_DIM;
use crate::error::RoobuError;
use crate::ui::{ui_step, ui_success};

const COLLECTION: &str = "roobu";

pub fn encode_point_id(site_ns: u64, post_id: u64) -> u64 {
	site_ns * 1_000_000_000_000 + post_id
}

pub fn decode_point_id(point_id: u64) -> (u64, u64) {
	(point_id / 1_000_000_000_000, point_id % 1_000_000_000_000)
}

pub struct PostEmbedding {
	pub post_id: u64,
	pub site: &'static str,
	pub site_namespace: u64,
	pub post_url: String,
	pub rating: String,
	pub image_vec: [f32; EMBED_DIM],
	pub tags_vec: [f32; EMBED_DIM],
}

pub struct Store {
	client: Qdrant,
}

impl Store {
	pub async fn new(url: &str) -> Result<Self, RoobuError> {
		let client = Qdrant::from_url(url).build().map_err(RoobuError::Qdrant)?;
		let store = Self { client };
		store.ensure_collection().await?;
		Ok(store)
	}

	async fn ensure_collection(&self) -> Result<(), RoobuError> {
		if self.client.collection_exists(COLLECTION).await? {
			tracing::debug!("collection '{COLLECTION}' exists");
			return Ok(());
		}

		ui_step!("Creating Qdrant collection '{COLLECTION}'…");

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
		vectors.add_named_vector_params(
			"tags",
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
			.create_collection(CreateCollectionBuilder::new(COLLECTION).vectors_config(vectors))
			.await?;

		self.client
			.create_field_index(
				CreateFieldIndexCollectionBuilder::new(COLLECTION, "post_id", FieldType::Integer)
					.wait(true),
			)
			.await?;

		self.client
			.create_field_index(
				CreateFieldIndexCollectionBuilder::new(COLLECTION, "site", FieldType::Keyword)
					.wait(true),
			)
			.await?;

		ui_success!("Collection ready");
		Ok(())
	}

	pub async fn upsert(&self, embeddings: Vec<PostEmbedding>) -> Result<(), RoobuError> {
		if embeddings.is_empty() {
			return Ok(());
		}

		let points: Vec<PointStruct> = embeddings
			.into_iter()
			.map(|e| {
				let point_id = encode_point_id(e.site_namespace, e.post_id);

				let vectors = NamedVectors::default()
					.add_vector("image", e.image_vec.to_vec())
					.add_vector("tags", e.tags_vec.to_vec());

				let payload: serde_json::Map<String, serde_json::Value> = [
					("post_id".to_string(), serde_json::json!(e.post_id as i64)),
					("site".to_string(), serde_json::json!(e.site)),
					("post_url".to_string(), serde_json::json!(e.post_url)),
					("rating".to_string(), serde_json::json!(e.rating)),
				]
				.into_iter()
				.collect();

				PointStruct::new(point_id, vectors, payload)
			})
			.collect();

		self.client
			.upsert_points(UpsertPointsBuilder::new(COLLECTION, points).wait(true))
			.await?;

		Ok(())
	}

	pub async fn search(
		&self,
		query_vec: Vec<f32>,
		image_weight: f32,
		tags_weight: f32,
		limit: u64,
		site_filter: Option<&str>,
	) -> Result<Vec<SearchResult>, RoobuError> {
		let fetch_limit = limit * 3;

		let filter =
			site_filter.map(|site| Filter::must([Condition::matches("site", site.to_string())]));

		let mut image_search = SearchPointsBuilder::new(COLLECTION, query_vec.clone(), fetch_limit)
			.vector_name("image")
			.with_payload(true);

		let mut tags_search = SearchPointsBuilder::new(COLLECTION, query_vec, fetch_limit)
			.vector_name("tags")
			.with_payload(true);

		if let Some(ref f) = filter {
			image_search = image_search.filter(f.clone());
			tags_search = tags_search.filter(f.clone());
		}

		let (image_resp, tags_resp): (SearchResponse, SearchResponse) = tokio::try_join!(
			async {
				self.client
					.search_points(image_search)
					.await
					.map_err(RoobuError::Qdrant)
			},
			async {
				self.client
					.search_points(tags_search)
					.await
					.map_err(RoobuError::Qdrant)
			},
		)?;

		let mut merged: std::collections::HashMap<u64, SearchResult> =
			std::collections::HashMap::new();

		for point in image_resp.result {
			let Some(id) = point
				.id
				.as_ref()
				.and_then(|pid| pid.point_id_options.as_ref())
			else {
				continue;
			};
			let qdrant_client::qdrant::point_id::PointIdOptions::Num(point_id) = id else {
				continue;
			};
			let post_url = point
				.payload
				.get("post_url")
				.and_then(|v| v.as_str())
				.map_or("", |v| v)
				.to_string();
			let (_, raw_post_id) = decode_point_id(*point_id);

			let entry = merged.entry(*point_id).or_insert_with(|| SearchResult {
				point_id: *point_id,
				post_id: raw_post_id,
				post_url,
				score: 0.0,
			});
			entry.score += image_weight * point.score;
		}

		for point in tags_resp.result {
			let Some(id) = point
				.id
				.as_ref()
				.and_then(|pid| pid.point_id_options.as_ref())
			else {
				continue;
			};
			let qdrant_client::qdrant::point_id::PointIdOptions::Num(point_id) = id else {
				continue;
			};
			let post_url = point
				.payload
				.get("post_url")
				.and_then(|v| v.as_str())
				.map_or("", |v| v)
				.to_string();
			let (_, raw_post_id) = decode_point_id(*point_id);

			let entry = merged.entry(*point_id).or_insert_with(|| SearchResult {
				point_id: *point_id,
				post_id: raw_post_id,
				post_url,
				score: 0.0,
			});
			entry.score += tags_weight * point.score;
		}

		let mut results: Vec<SearchResult> = merged.into_values().collect();
		results.sort_by(|a, b| {
			b.score
				.partial_cmp(&a.score)
				.unwrap_or(std::cmp::Ordering::Equal)
		});
		results.truncate(limit as usize);
		Ok(results)
	}
}

pub struct SearchResult {
	pub point_id: u64,
	pub post_id: u64,
	pub post_url: String,
	pub score: f32,
}
