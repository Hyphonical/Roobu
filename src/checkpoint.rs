use std::collections::HashMap;
use std::path::Path;

use crate::error::RoobuError;

pub type CheckpointMap = HashMap<String, u64>;

pub fn load(path: &Path) -> CheckpointMap {
	let Ok(data) = std::fs::read_to_string(path) else {
		return CheckpointMap::new();
	};
	serde_json::from_str(&data).unwrap_or_default()
}

pub fn save(path: &Path, map: &CheckpointMap) -> Result<(), RoobuError> {
	let tmp = path.with_extension("tmp");
	let json = serde_json::to_string_pretty(map)
		.map_err(|e| RoobuError::Api(format!("checkpoint serialize: {e}")))?;
	std::fs::write(&tmp, json)?;
	std::fs::rename(&tmp, path)?;
	Ok(())
}

pub fn get(map: &CheckpointMap, site: &str) -> u64 {
	map.get(site).copied().unwrap_or(0)
}

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
}
