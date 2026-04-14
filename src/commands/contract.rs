//! Contract command - freeze and validate the OpenAPI API contract.

use std::path::Path;

use anyhow::{Context, bail};

use crate::cli::ContractCommand;
use crate::ui::header;
use crate::web;
use crate::{ui_step, ui_success};

pub async fn run(command: ContractCommand) -> anyhow::Result<()> {
	header("roobu · contract");

	match command {
		ContractCommand::Export { output } => export_snapshot(&output),
		ContractCommand::Check { snapshot } => check_snapshot(&snapshot),
	}
}

fn export_snapshot(output: &Path) -> anyhow::Result<()> {
	ui_step!("Generating OpenAPI document");
	let json = normalized_openapi_json()?;

	if let Some(parent) = output.parent() {
		std::fs::create_dir_all(parent).with_context(|| {
			format!(
				"failed to create contract output directory {}",
				parent.display()
			)
		})?;
	}

	std::fs::write(output, json)
		.with_context(|| format!("failed to write OpenAPI snapshot to {}", output.display()))?;

	ui_success!("Wrote OpenAPI snapshot to {}", output.display());
	Ok(())
}

fn check_snapshot(snapshot: &Path) -> anyhow::Result<()> {
	ui_step!(
		"Comparing generated OpenAPI document against {}",
		snapshot.display()
	);

	let snapshot_json = std::fs::read_to_string(snapshot)
		.with_context(|| format!("failed to read snapshot {}", snapshot.display()))?;
	let snapshot_value = normalize_json(
		serde_json::from_str::<serde_json::Value>(&snapshot_json)
			.with_context(|| format!("snapshot {} is not valid JSON", snapshot.display()))?,
	);

	let current_value = normalize_json(serde_json::to_value(web::openapi_document())?);

	if snapshot_value != current_value {
		bail!(
			"OpenAPI contract drift detected. Run `roobu contract export --output {}` and regenerate typed frontend client output.",
			snapshot.display()
		);
	}

	ui_success!("OpenAPI snapshot matches generated contract");
	Ok(())
}

fn normalized_openapi_json() -> anyhow::Result<String> {
	let value = normalize_json(serde_json::to_value(web::openapi_document())?);
	serde_json::to_string_pretty(&value).context("failed to serialize OpenAPI JSON")
}

fn normalize_json(value: serde_json::Value) -> serde_json::Value {
	match value {
		serde_json::Value::Object(map) => {
			let mut keys: Vec<String> = map.keys().cloned().collect();
			keys.sort();
			let mut normalized = serde_json::Map::new();
			for key in keys {
				if let Some(child) = map.get(&key) {
					normalized.insert(key, normalize_json(child.clone()));
				}
			}
			serde_json::Value::Object(normalized)
		}
		serde_json::Value::Array(items) => {
			serde_json::Value::Array(items.into_iter().map(normalize_json).collect())
		}
		other => other,
	}
}
