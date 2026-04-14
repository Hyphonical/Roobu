//! Qdrant vector database client and schema management.
//!
//! Provides the [`Store`] struct for interacting with Qdrant, including
//! collection creation, point upsertion, semantic search, vector fetching
//! for clustering, and site distribution statistics.

mod cluster;
mod schema;
mod search;
mod stats;

pub use schema::SiteDistribution;

use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
	CreateCollectionBuilder, CreateFieldIndexCollectionBuilder, Distance, FieldType, NamedVectors,
	PointStruct, QuantizationType, ScalarQuantizationBuilder, UpsertPointsBuilder,
	VectorParamsBuilder, VectorsConfigBuilder,
};

use crate::config;
use crate::embed::EMBED_DIM;
use crate::error::RoobuError;
use crate::{ui_step, ui_success};

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
	pub site: String,
	pub post_url: String,
	pub thumbnail_url: String,
	pub direct_image_url: String,
	pub width: u32,
	pub height: u32,
	pub ingestion_date: i64,
	pub score: f32,
	pub tags: String,
	pub rating: String,
}

/// A paginated page of recent posts.
pub struct RecentPage {
	pub posts: Vec<SearchResult>,
	pub has_more: bool,
}

/// A point fetched from Qdrant for clustering, containing its embedding vector.
pub struct ClusterPoint {
	pub post_id: u64,
	pub post_url: String,
	pub image_vec: Vec<f32>,
}

/// Metadata about an indexed site.
pub struct SiteInfo {
	pub name: String,
	#[allow(dead_code)]
	pub namespace: u64,
	pub count: u64,
	pub earliest_ingestion: i64,
	pub latest_ingestion: i64,
}

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
				let point_id = schema::encode_point_id(e.site_namespace, e.post_id);
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
	///
	/// `site_filter` is a slice of site name strings. An empty slice means no
	/// filtering (all sites). Multiple entries are combined with OR semantics.
	pub async fn search(
		&self,
		query_vec: Vec<f32>,
		limit: u64,
		site_filter: &[&str],
	) -> Result<Vec<SearchResult>, RoobuError> {
		search::search(&self.client, query_vec, limit, site_filter).await
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
		cluster::fetch_image_vectors(&self.client, site_filter, page_size, max_points).await
	}

	/// Compute the distribution of indexed points across all sites.
	pub async fn fetch_site_counts(&self, page_size: u32) -> Result<SiteDistribution, RoobuError> {
		stats::fetch_site_counts(&self.client, page_size).await
	}

	/// Fetch a single post by site name and post ID.
	pub async fn fetch_post(
		&self,
		site: &str,
		post_id: u64,
	) -> Result<Option<SearchResult>, RoobuError> {
		search::fetch_post(&self.client, site, post_id).await
	}

	/// Find posts similar to a given post by using its embedding vector.
	pub async fn search_similar(
		&self,
		site: &str,
		post_id: u64,
		limit: u64,
	) -> Result<Vec<SearchResult>, RoobuError> {
		search::search_similar(&self.client, site, post_id, limit).await
	}

	/// Fetch the most recently ingested posts.
	///
	/// Uses a range filter on `ingestion_date` for efficient retrieval,
	/// then paginates with an offset. Returns a page with a `has_more` flag.
	pub async fn fetch_recent(
		&self,
		limit: u64,
		offset: u64,
		site_filter: &[&str],
	) -> Result<RecentPage, RoobuError> {
		search::fetch_recent(&self.client, limit, offset, site_filter).await
	}

	/// Fetch ingestion activity data: daily counts over the last N days.
	pub async fn fetch_activity(
		&self,
		days: u64,
		site_filter: &[&str],
	) -> Result<Vec<(String, u64)>, RoobuError> {
		stats::fetch_activity(&self.client, days, site_filter).await
	}

	/// Fetch metadata for all indexed sites.
	pub async fn fetch_sites(&self) -> Result<Vec<SiteInfo>, RoobuError> {
		stats::fetch_sites(&self.client).await
	}
}
