# Adding a New Site

This guide walks through adding a new adapter in a way that matches current project patterns.

## Prerequisites

Before starting:

1. Confirm the site has a stable API endpoint for recent content.
2. Confirm each item exposes:
   - numeric id
   - image URL suitable for embedding
   - optional width/height
   - tags or metadata text
3. Decide whether authentication is required.

## Step 1: Create Site Module

Add a new file in src/sites, for example:

- src/sites/example.rs

Implement:

- ExampleClient struct with reqwest::Client
- constructor with user-agent and timeout
- fetch logic with retry/backoff
- JSON mapping structs
- conversion from raw API item to sites::Post

Recommended constants:

- SITE_NAME as payload string
- SITE_NAMESPACE as unique u64
- endpoint URL and retry/backoff limits

## Step 2: Implement BooruClient

Implement for your client:

- site_name
- fetch_recent(last_id)
- download_preview(url)

Behavioral expectations:

- fetch_recent should return only posts newer than last_id
- return empty vector on no new data
- map API/parse failures to RoobuError::Api where relevant
- include reasonable retries for 429 and server errors

## Step 3: Wire Module in src/sites/mod.rs

Update all of the following:

1. Add module declaration:
   - pub mod example;
2. Add SiteKind variant:
   - Example
3. Add SiteClient variant:
   - Example(example::ExampleClient)
4. Extend build_client match arm.
5. Extend post URL mapping in Post::post_url when needed.
6. Extend SiteClient dispatch matches for:
   - site_name
   - fetch_recent
   - download_preview

Important:

- choose a unique SITE_NAMESPACE not used by any existing site
- keep payload site string stable because it is used for filtering and checkpoints

## Step 4: Wire All-sites Mode

In src/commands/ingest.rs:

- add your site client in build_all_sites_clients order
- if auth is required, apply the same credential-pair pattern used by rule34/gelbooru
- update tests that assert the exact all-sites site-name list

## Step 5: Update CLI Help and Docs

Update:

- src/cli.rs
  - --site help text list
- README.md
  - site selector list and examples as needed
- docs/sites.md
  - matrix row for the new site

## Step 6: Add Tests

At minimum:

- module-level raw JSON to Post mapping tests in your new site file
- fallback behavior tests for missing image URLs and metadata
- update list/order tests in src/commands/ingest.rs
- update URL mapping tests in src/sites/mod.rs if post URL behavior changed

Good test targets:

- URL fallback order
- rating mapping
- tag extraction and normalization assumptions
- credential validation (if applicable)

## Step 7: Validate Quality Gates

Run:

1. cargo fmt
2. cargo check
3. cargo clippy --all-targets --all-features
4. cargo test

Do not merge a new site adapter while warnings are present.

## Design Guidelines

### Keep Adapter Logic Local

Site-specific parsing should stay in the site module. Avoid leaking site quirks into ingest/store code.

### Prefer Robust Fallbacks

When an API has multiple image URL candidates, implement explicit fallback order.

### Keep Tags Useful

If a site lacks classic booru tags, synthesize searchable text from available metadata fields.

### Preserve Stability

Checkpoint progression depends on numeric post ids. Use stable identifiers and avoid deriving ids from mutable fields.

## Minimal Skeleton

```rust
const SITE_NAME: &str = "example";
const SITE_NAMESPACE: u64 = 13;

pub struct ExampleClient {
    http: reqwest::Client,
}

impl ExampleClient {
    pub fn new() -> Result<Self, RoobuError> { /* ... */ }
}

impl BooruClient for ExampleClient {
    fn site_name(&self) -> &'static str { SITE_NAME }

    async fn fetch_recent(&self, last_id: u64) -> Result<Vec<Post>, RoobuError> {
        /* ... */
    }

    async fn download_preview(&self, url: &str) -> Result<bytes::Bytes, RoobuError> {
        /* ... */
    }
}
```

## Pull Request Checklist

- New adapter file added with tests.
- src/sites/mod.rs fully wired.
- src/commands/ingest.rs updated for all-sites mode.
- src/cli.rs and docs updated.
- All quality gates pass without warnings.
