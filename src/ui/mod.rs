//! Terminal UI helpers for consistent, colorized output.
//!
//! Provides a set of functions for printing styled messages
//! to the terminal during CLI operations. Macros are defined in [`macros`].

mod macros;

use std::fmt;

use owo_colors::OwoColorize;

/// Print a section header with a decorative line.
pub fn header(label: &str) {
	let width: usize = 56;
	let inner = format!("── {label} ");
	let pad = width.saturating_sub(inner.len());
	let line = format!("{inner}{}", "─".repeat(pad));
	println!("\n{line}\n");
}

/// Print a step indicator (●) for the current operation.
pub fn step(msg: fmt::Arguments<'_>) {
	println!("{} {msg}", "●".cyan().bold());
}

/// Print a detail line with a labeled value.
pub fn detail(label: &str, value: fmt::Arguments<'_>) {
	println!("  {} {label:<9}{value}", "·".dimmed());
}

/// Print a success indicator (✓).
pub fn success(msg: fmt::Arguments<'_>) {
	println!("{} {msg}", "✓".green());
}

/// Print a warning indicator (⚠).
pub fn warn(msg: fmt::Arguments<'_>) {
	println!("{} {msg}", "⚠".yellow());
}

/// Print a failure indicator (✗).
#[allow(dead_code)]
pub fn fail(msg: fmt::Arguments<'_>) {
	println!("{} {msg}", "✗".red().bold());
}

/// Print a dimmed sub-detail line.
#[allow(dead_code)]
pub fn sub_detail(msg: fmt::Arguments<'_>) {
	println!("  {}", msg.to_string().dimmed());
}
