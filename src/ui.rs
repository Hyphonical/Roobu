use owo_colors::OwoColorize;
use std::fmt;

pub fn header(label: &str) {
	let width: usize = 56;
	let inner = format!("── {} ", label);
	let pad = width.saturating_sub(inner.len());
	let line = format!("{}{}", inner, "─".repeat(pad));
	println!("\n{}\n", line.dimmed());
}

pub fn step(msg: fmt::Arguments<'_>) {
	println!("{} {}", "●".cyan().bold(), msg);
}

pub fn detail(label: &str, value: fmt::Arguments<'_>) {
	println!("  {} {label:<9}{}", "·".dimmed(), value);
}

pub fn success(msg: fmt::Arguments<'_>) {
	println!("{} {}", "✓".green(), msg);
}

pub fn warn(msg: fmt::Arguments<'_>) {
	println!("{} {}", "⚠".yellow(), msg);
}

pub fn fail(msg: fmt::Arguments<'_>) {
	println!("{} {}", "✗".red().bold(), msg);
}

pub fn sub_detail(msg: fmt::Arguments<'_>) {
	println!("  {}", msg.to_string().dimmed());
}

#[allow(unused_macros)]
macro_rules! ui_step {
    ($($arg:tt)*) => { $crate::ui::step(format_args!($($arg)*)) };
}

#[allow(unused_macros)]
macro_rules! ui_detail {
    ($label:expr, $($arg:tt)*) => { $crate::ui::detail($label, format_args!($($arg)*)) };
}

#[allow(unused_macros)]
macro_rules! ui_success {
    ($($arg:tt)*) => { $crate::ui::success(format_args!($($arg)*)) };
}

#[allow(unused_macros)]
macro_rules! ui_warn {
    ($($arg:tt)*) => { $crate::ui::warn(format_args!($($arg)*)) };
}

#[allow(unused_macros)]
macro_rules! ui_fail {
    ($($arg:tt)*) => { $crate::ui::fail(format_args!($($arg)*)) };
}

#[allow(unused_macros)]
macro_rules! ui_sub {
    ($($arg:tt)*) => { $crate::ui::sub_detail(format_args!($($arg)*)) };
}

pub(crate) use {ui_detail, ui_step, ui_success, ui_warn};
