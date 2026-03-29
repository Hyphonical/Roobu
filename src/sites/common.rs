pub fn normalize_url(value: Option<String>) -> Option<String> {
	value.and_then(|url| {
		let trimmed = url.trim();
		if trimmed.is_empty() {
			None
		} else {
			Some(trimmed.to_string())
		}
	})
}

pub fn first_url(candidates: impl IntoIterator<Item = Option<String>>) -> Option<String> {
	candidates.into_iter().find_map(normalize_url)
}

pub fn first_url_or_empty(candidates: impl IntoIterator<Item = Option<String>>) -> String {
	first_url(candidates).unwrap_or_default()
}
