//! Site distribution statistics.

use std::collections::BTreeMap;

use qdrant_client::Qdrant;
use qdrant_client::qdrant::ScrollPointsBuilder;

use super::schema::SiteDistribution;
use crate::config;
use crate::error::RoobuError;

/// Compute the distribution of indexed points across all sites.
pub async fn fetch_site_counts(
	client: &Qdrant,
	page_size: u32,
) -> Result<SiteDistribution, RoobuError> {
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

		let response = client.scroll(request).await?;
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
