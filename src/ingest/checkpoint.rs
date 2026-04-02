//! Ingest checkpoint management for resume support.
//!
//! The checkpoint file stores the last successfully processed post ID per site,
//! enabling the ingest loop to resume from where it left off after a restart
//! or interruption. Uses atomic file writes to prevent corruption.

use std::collections::HashMap;
use std::path::Path;

use crate::error::RoobuError;

/// A map from site name to the last successfully processed post ID.
pub type CheckpointMap = HashMap<String, u64>;

/// Load the checkpoint from disk, returning an empty map on failure.
///
/// If the file doesn't exist or contains malformed JSON, a warning is logged
/// and an empty map is returned (effectively restarting from scratch).
pub fn load(path: &Path) -> CheckpointMap {
	let Ok(data) = std::fs::read_to_string(path) else {
		return CheckpointMap::new();
	};
	match serde_json::from_str(&data) {
		Ok(map) => map,
		Err(e) => {
			tracing::warn!(
				path = %path.display(),
				error = %e,
				"checkpoint file is malformed; starting from scratch"
			);
			CheckpointMap::new()
		}
	}
}

/// Atomically save the checkpoint to disk.
///
/// Writes to a temporary file first, then renames it into place. This
/// prevents corruption if the process is interrupted during the write.
pub fn save(path: &Path, map: &CheckpointMap) -> Result<(), RoobuError> {
	let json = serde_json::to_string_pretty(map)
		.map_err(|e| RoobuError::Api(format!("checkpoint serialize: {e}")))?;

	let parent = path.parent().unwrap_or_else(|| Path::new("."));
	let nanos = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default()
		.as_nanos();
	let tmp = parent.join(format!(".checkpoint.tmp.{}.{}", std::process::id(), nanos));

	std::fs::write(&tmp, json)?;

	// On Windows, rename over an existing file can fail. Try rename first,
	// fall back to remove+rename for cross-platform compatibility.
	if path.exists() {
		if std::fs::rename(&tmp, path).is_err() {
			std::fs::remove_file(path)?;
			std::fs::rename(&tmp, path)?;
		}
	} else {
		std::fs::rename(&tmp, path)?;
	}

	Ok(())
}

/// Get the last processed post ID for a site, defaulting to 0.
pub fn get(map: &CheckpointMap, site: &str) -> u64 {
	map.get(site).copied().unwrap_or(0)
}

/// Set the last processed post ID for a site in the checkpoint map.
pub fn set(map: &mut CheckpointMap, site: &str, id: u64) {
	map.insert(site.to_string(), id);
}

#[cfg(test)]
mod tests {
	use std::time::{SystemTime, UNIX_EPOCH};

	use super::{CheckpointMap, get, load, save, set};

	#[test]
	fn get_and_set_are_site_scoped() {
		let mut map = CheckpointMap::new();
		set(&mut map, "rule34", 100);
		set(&mut map, "e621", 200);

		assert_eq!(get(&map, "rule34"), 100);
		assert_eq!(get(&map, "e621"), 200);
		assert_eq!(get(&map, "missing"), 0);
	}

	#[test]
	fn save_and_load_roundtrip() {
		let unique = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.expect("time went backwards")
			.as_nanos();
		let path = std::env::temp_dir().join(format!("roobu-checkpoint-{unique}.json"));

		let mut map = CheckpointMap::new();
		set(&mut map, "rule34", 1234);
		set(&mut map, "e621", 5678);

		save(&path, &map).expect("save should succeed");
		let loaded = load(&path);

		assert_eq!(get(&loaded, "rule34"), 1234);
		assert_eq!(get(&loaded, "e621"), 5678);

		let _ = std::fs::remove_file(path);
	}

	#[test]
	fn load_returns_empty_map_for_corrupted_file() {
		let unique = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.expect("time went backwards")
			.as_nanos();
		let path = std::env::temp_dir().join(format!("roobu-checkpoint-corrupt-{unique}.json"));

		std::fs::write(&path, "this is not valid json {{{").expect("write should succeed");
		let loaded = load(&path);

		assert!(
			loaded.is_empty(),
			"corrupted checkpoint should yield empty map"
		);

		let _ = std::fs::remove_file(path);
	}

	#[test]
	fn load_returns_empty_map_for_missing_file() {
		let path = std::env::temp_dir().join("roobu-checkpoint-nonexistent-12345.json");
		let loaded = load(&path);
		assert!(
			loaded.is_empty(),
			"missing checkpoint should yield empty map"
		);
	}

	#[test]
	fn save_overwrites_existing_file() {
		let unique = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.expect("time went backwards")
			.as_nanos();
		let path = std::env::temp_dir().join(format!("roobu-checkpoint-overwrite-{unique}.json"));

		// First save
		let mut map1 = CheckpointMap::new();
		set(&mut map1, "rule34", 100);
		save(&path, &map1).expect("first save should succeed");

		// Overwrite with different data
		let mut map2 = CheckpointMap::new();
		set(&mut map2, "rule34", 999);
		save(&path, &map2).expect("second save should succeed");

		let loaded = load(&path);
		assert_eq!(
			get(&loaded, "rule34"),
			999,
			"overwrite should replace value"
		);

		let _ = std::fs::remove_file(path);
	}
}
