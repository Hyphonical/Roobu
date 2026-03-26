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
