//! Terminal UI helpers for consistent, colorized output.
//!
//! Provides a set of functions and macros for printing styled messages
//! to the terminal during CLI operations.

use std::fmt;

use owo_colors::OwoColorize;

// ── Public Functions ────────────────────────────────────────────────────────

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

// ── Convenience Macros ──────────────────────────────────────────────────────

/// Format and print a step message.
#[macro_export]
macro_rules! ui_step {
	($($arg:tt)*) => {
		$crate::ui::step(format_args!($($arg)*))
	};
}

/// Format and print a detail line with a label.
#[macro_export]
macro_rules! ui_detail {
	($label:expr, $($arg:tt)*) => {
		$crate::ui::detail($label, format_args!($($arg)*))
	};
}

/// Format and print a success message.
#[macro_export]
macro_rules! ui_success {
	($($arg:tt)*) => {
		$crate::ui::success(format_args!($($arg)*))
	};
}

/// Format and print a warning message.
#[macro_export]
macro_rules! ui_warn {
	($($arg:tt)*) => {
		$crate::ui::warn(format_args!($($arg)*))
	};
}

/// Format and print a failure message.
#[macro_export]
macro_rules! ui_fail {
	($($arg:tt)*) => {
		$crate::ui::fail(format_args!($($arg)*))
	};
}

/// Format and print a dimmed sub-detail message.
#[macro_export]
macro_rules! ui_sub {
	($($arg:tt)*) => {
		$crate::ui::sub_detail(format_args!($($arg)*))
	};
}
