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
use crate::ui::{ui_step, ui_success};

pub fn encode_point_id(site_ns: u64, post_id: u64) -> u64 {
	site_ns * config::POINT_ID_SITE_MULTIPLIER + post_id
}

pub fn decode_point_id(point_id: u64) -> (u64, u64) {
	(
		point_id / config::POINT_ID_SITE_MULTIPLIER,
		point_id % config::POINT_ID_SITE_MULTIPLIER,
	)
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
		let client = Qdrant::from_url(url).build().map_err(RoobuError::from)?;
		let store = Self { client };
		store.ensure_collection().await?;
		Ok(store)
	}

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
			.create_collection(
				CreateCollectionBuilder::new(config::QDRANT_COLLECTION).vectors_config(vectors),
			)
			.await?;

		self.client
			.create_field_index(
				CreateFieldIndexCollectionBuilder::new(
					config::QDRANT_COLLECTION,
					"post_id",
					FieldType::Integer,
				)
				.wait(true),
			)
			.await?;

		self.client
			.create_field_index(
				CreateFieldIndexCollectionBuilder::new(
					config::QDRANT_COLLECTION,
					"site",
					FieldType::Keyword,
				)
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
			.upsert_points(UpsertPointsBuilder::new(config::QDRANT_COLLECTION, points).wait(true))
			.await?;

		Ok(())
	}

	pub async fn search(
		&self,
		image_query_vec: Option<Vec<f32>>,
		tags_query_vec: Option<Vec<f32>>,
		image_weight: f32,
		tags_weight: f32,
		limit: u64,
		site_filter: Option<&str>,
	) -> Result<Vec<SearchResult>, RoobuError> {
		let fetch_limit = limit.saturating_mul(config::SEARCH_FETCH_LIMIT_MULTIPLIER);

		let filter =
			site_filter.map(|site| Filter::must([Condition::matches("site", site.to_string())]));

		let mut image_search = image_query_vec.map(|query| {
			SearchPointsBuilder::new(config::QDRANT_COLLECTION, query, fetch_limit)
				.vector_name("image")
				.with_payload(true)
		});

		let mut tags_search = tags_query_vec.map(|query| {
			SearchPointsBuilder::new(config::QDRANT_COLLECTION, query, fetch_limit)
				.vector_name("tags")
				.with_payload(true)
		});

		if let Some(ref f) = filter {
			if let Some(search) = image_search.take() {
				image_search = Some(search.filter(f.clone()));
			}
			if let Some(search) = tags_search.take() {
				tags_search = Some(search.filter(f.clone()));
			}
		}

		let run_image = image_weight > 0.0 && image_search.is_some();
		let run_tags = tags_weight > 0.0 && tags_search.is_some();

		let (image_points, tags_points) = match (run_image, run_tags) {
			(true, true) => {
				let image_builder = image_search.expect("checked above");
				let tags_builder = tags_search.expect("checked above");
				tokio::try_join!(
					async {
						let response: SearchResponse = self
							.client
							.search_points(image_builder)
							.await
							.map_err(RoobuError::from)?;
						Ok::<_, RoobuError>(response.result)
					},
					async {
						let response: SearchResponse = self
							.client
							.search_points(tags_builder)
							.await
							.map_err(RoobuError::from)?;
						Ok::<_, RoobuError>(response.result)
					},
				)?
			}
			(true, false) => {
				let response: SearchResponse = self
					.client
					.search_points(image_search.expect("checked above"))
					.await
					.map_err(RoobuError::from)?;
				(response.result, Vec::new())
			}
			(false, true) => {
				let response: SearchResponse = self
					.client
					.search_points(tags_search.expect("checked above"))
					.await
					.map_err(RoobuError::from)?;
				(Vec::new(), response.result)
			}
			(false, false) => return Ok(Vec::new()),
		};

		let mut merged: std::collections::HashMap<u64, SearchResult> =
			std::collections::HashMap::new();

		for point in image_points {
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
				post_id: raw_post_id,
				post_url,
				score: 0.0,
			});
			entry.score += image_weight * point.score;
		}

		for point in tags_points {
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
				.with_payload(false)
				.with_vectors(VectorsSelector {
					names: vec!["image".to_string()],
				});

			if let Some(ref f) = filter {
				request = request.filter(f.clone());
			}

			if let Some(current_offset) = offset.clone() {
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

				let PointIdOptions::Num(_) = id else {
					continue;
				};

				let Some(vectors) = point.vectors.as_ref() else {
					continue;
				};

				let Some(image_vec) = extract_named_dense_vector(vectors, "image") else {
					continue;
				};

				if image_vec.len() != EMBED_DIM {
					continue;
				}

				points.push(ClusterPoint { image_vec });

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
}

pub struct SearchResult {
	pub post_id: u64,
	pub post_url: String,
	pub score: f32,
}

pub struct ClusterPoint {
	pub image_vec: Vec<f32>,
}

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
