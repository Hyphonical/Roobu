use anyhow::ensure;
use owo_colors::OwoColorize;

use crate::store;
use crate::ui::{header, ui_step, ui_success, ui_warn};

pub struct Args {
	pub qdrant_url: String,
	pub page_size: u32,
	pub width: usize,
}

pub async fn run(args: Args) -> anyhow::Result<()> {
	header("roobu · stats");

	ensure!(args.page_size > 0, "--page-size must be greater than 0");
	ensure!(args.width > 0, "--width must be greater than 0");

	ui_step!("{}", "Connecting to Qdrant…");
	let store = store::Store::new(&args.qdrant_url).await?;
	ui_success!("Qdrant ready");

	ui_step!(
		"{}",
		format!(
			"Scanning collection for site distribution (page size {})…",
			args.page_size
		)
		.as_str()
	);

	let distribution = store.fetch_site_counts(args.page_size).await?;
	if distribution.total_points == 0 {
		ui_warn!("No indexed posts found");
		return Ok(());
	}

	let mut rows: Vec<(String, u64)> = distribution.per_site.into_iter().collect();
	rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

	let site_count = rows.len();
	ui_success!(
		"{}",
		format!(
			"{} indexed posts across {} site{}",
			distribution.total_points.bold().bright_white(),
			site_count.bold().bright_white(),
			if site_count == 1 { "" } else { "s" }
		)
		.as_str()
	);

	println!();
	ui_step!("{}", "Per-site share");

	for (site, count) in &rows {
		let percent = (*count as f64 / distribution.total_points as f64) * 100.0;
		let bar_len = ((percent / 100.0) * args.width as f64).round() as usize;
		let bar_len = bar_len.max(1);
		let bar = "#".repeat(bar_len);

		println!(
			"  {:<10} {:>10}  {:>6.2}%  {}",
			site,
			count,
			percent,
			bar.cyan()
		);
	}

	if distribution.missing_site_payload > 0 {
		println!();
		ui_warn!(
			"{}",
			format!(
				"{} points are missing the 'site' payload field",
				distribution.missing_site_payload
			)
			.as_str()
		);
	}

	if let Some((leader_site, leader_count)) = rows.first() {
		let leader_pct = (*leader_count as f64 / distribution.total_points as f64) * 100.0;
		println!();
		ui_success!(
			"{}",
			format!(
				"Leader: {} ({leader_pct:.2}%)",
				leader_site.bright_white().bold()
			)
			.as_str()
		);
	}

	if rows.len() > 1
		&& let Some((tail_site, tail_count)) = rows.last()
	{
		let tail_pct = (*tail_count as f64 / distribution.total_points as f64) * 100.0;
		ui_step!(
			"{}",
			format!(
				"Trailing site: {} ({tail_pct:.2}%)",
				tail_site.bright_white()
			)
			.as_str()
		);
	}

	Ok(())
}
