//! Site distribution statistics.

use std::collections::BTreeMap;

use qdrant_client::Qdrant;
use qdrant_client::qdrant::ScrollPointsBuilder;

use super::SiteInfo;
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

/// Fetch ingestion activity data: daily counts over the last N days.
///
/// Returns a vector of (date_string, count) pairs sorted chronologically.
/// Dates are in ISO 8601 format (YYYY-MM-DD).
pub async fn fetch_activity(
	client: &Qdrant,
	days: u64,
	site_filter: &[&str],
) -> Result<Vec<(String, u64)>, RoobuError> {
	if days == 0 || days > 3650 {
		return Ok(Vec::new());
	}

	let filter = if site_filter.is_empty() {
		None
	} else if site_filter.len() == 1 {
		Some(qdrant_client::qdrant::Filter::must([
			qdrant_client::qdrant::Condition::matches("site", site_filter[0].to_string()),
		]))
	} else {
		let conditions: Vec<qdrant_client::qdrant::Condition> = site_filter
			.iter()
			.map(|s| qdrant_client::qdrant::Condition::matches("site", s.to_string()))
			.collect();
		Some(qdrant_client::qdrant::Filter::should(conditions))
	};

	// Calculate the cutoff timestamp (days ago).
	let now = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default()
		.as_secs() as i64;
	let cutoff = now - (days as i64 * 86_400);

	// Initialize daily buckets.
	let mut daily_counts: BTreeMap<String, u64> = BTreeMap::new();
	for i in (0..days).rev() {
		let ts = now - (i as i64 * 86_400);
		let date = unix_ts_to_date(ts);
		daily_counts.insert(date, 0);
	}

	// Scroll through all points and bucket by ingestion_date.
	let mut offset = None;
	let page_size: u32 = 1024;

	loop {
		let mut request = ScrollPointsBuilder::new(config::QDRANT_COLLECTION)
			.limit(page_size)
			.with_payload(true);

		if let Some(current_offset) = offset {
			request = request.offset(current_offset);
		}

		if let Some(ref f) = filter {
			request = request.filter(f.clone());
		}

		let response = client.scroll(request).await?;
		let next_offset = response.next_page_offset;

		if response.result.is_empty() {
			break;
		}

		for point in &response.result {
			let ingestion_date = super::schema::payload_i64(&point.payload, "ingestion_date");
			if ingestion_date >= cutoff {
				let date = unix_ts_to_date(ingestion_date);
				*daily_counts.entry(date).or_insert(0) += 1;
			}
		}

		offset = next_offset;
		if offset.is_none() {
			break;
		}
	}

	Ok(daily_counts.into_iter().collect())
}

/// Fetch metadata for all indexed sites.
pub async fn fetch_sites(client: &Qdrant) -> Result<Vec<SiteInfo>, RoobuError> {
	let mut offset = None;
	let page_size: u32 = 1024;

	// Track per-site: count, earliest, latest.
	let mut site_data: BTreeMap<String, (u64, i64, i64)> = BTreeMap::new();

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

		for point in &response.result {
			let site = super::schema::payload_string(&point.payload, "site");
			if site.is_empty() {
				continue;
			}

			let ingestion_date = super::schema::payload_i64(&point.payload, "ingestion_date");

			let entry = site_data.entry(site).or_insert((0, i64::MAX, i64::MIN));
			entry.0 += 1;
			if ingestion_date < entry.1 {
				entry.1 = ingestion_date;
			}
			if ingestion_date > entry.2 {
				entry.2 = ingestion_date;
			}
		}

		offset = next_offset;
		if offset.is_none() {
			break;
		}
	}

	// Map site names to their namespaces (from site adapter constants).
	let namespace_map = site_namespace_map();

	let sites: Vec<SiteInfo> = site_data
		.into_iter()
		.map(|(name, (count, earliest, latest))| SiteInfo {
			name: name.clone(),
			namespace: *namespace_map.get(&name).unwrap_or(&0),
			count,
			earliest_ingestion: earliest,
			latest_ingestion: latest,
		})
		.collect();

	Ok(sites)
}

/// Map site names to their Qdrant point ID namespaces.
fn site_namespace_map() -> BTreeMap<String, u64> {
	BTreeMap::from([
		("rule34".to_string(), 1),
		("e621".to_string(), 2),
		("safebooru".to_string(), 3),
		("gelbooru".to_string(), 4),
		("danbooru".to_string(), 5),
		("xbooru".to_string(), 6),
		("kemono".to_string(), 7),
		("aibooru".to_string(), 8),
		("e6ai".to_string(), 9),
		("konachan".to_string(), 10),
		("yandere".to_string(), 11),
		("civitai".to_string(), 12),
	])
}

/// Convert a Unix timestamp to an ISO 8601 date string (YYYY-MM-DD).
fn unix_ts_to_date(ts: i64) -> String {
	let secs = ts.max(0) as u64;
	let days_since_epoch = secs / 86_400;

	// Calculate year, month, day from days since Unix epoch.
	let mut days = days_since_epoch as i64;
	let mut year = 1970i64;

	loop {
		let days_in_year = if is_leap_year(year) { 366 } else { 365 };
		if days < days_in_year {
			break;
		}
		days -= days_in_year;
		year += 1;
	}

	let month_lengths = if is_leap_year(year) {
		[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
	} else {
		[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
	};

	let mut month = 0;
	for (i, &ml) in month_lengths.iter().enumerate() {
		if days < ml as i64 {
			month = i;
			break;
		}
		days -= ml as i64;
	}

	let day = days + 1;
	format!("{:04}-{:02}-{:02}", year, month + 1, day)
}

fn is_leap_year(year: i64) -> bool {
	(year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
