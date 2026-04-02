//! Convenience macros for terminal UI output.
//!
//! These macros wrap the [`super`] functions with `format_args!` for
//! convenient formatted output.

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
