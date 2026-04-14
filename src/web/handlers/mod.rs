//! REST API request handlers for the web server.

pub mod activity;
pub mod common;
pub mod ingest;
pub mod posts;
pub mod recent;
pub mod search;
pub mod sites;

pub use activity::{ActivityDayDto, ActivityDto, ActivityParams, activity};
pub use common::{ApiResponse, ErrorDto, PostDto, ResponseMeta};
pub use ingest::{IngestStatusDto, ingest_status};
pub use posts::get_post;
pub use recent::{RecentParams, recent};
pub use search::{
	SearchParams, SearchUploadForm, SimilarParams, search, search_similar, search_upload,
};
pub use sites::{SiteDto, sites};
