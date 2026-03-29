# Sites

This page summarizes supported sources and integration behavior.

## Supported Site Kinds

- rule34
- e621
- safebooru
- xbooru
- kemono
- aibooru
- danbooru
- civitai
- e6ai
- gelbooru
- konachan
- yandere

## Site Matrix

| Site | Payload Name | Namespace | Credentials Required | Notes |
|---|---|---:|---|---|
| Rule34 | rule34 | 1 | Yes | Requires API key + user id. |
| e621 | e621 | 2 | Optional | If auth is used, login and API key must be paired. |
| Safebooru | safebooru | 3 | No | Public adapter. |
| Gelbooru | gelbooru | 4 | Yes | Requires API key + user id. |
| Danbooru | danbooru | 5 | No | Public adapter. |
| Xbooru | xbooru | 6 | No | Public adapter. |
| Kemono | kemono | 7 | Optional | Session/base-url can improve freshness/fallback behavior. |
| Aibooru | aibooru | 8 | No | Public adapter. |
| e6ai | e6ai | 9 | No | Public adapter. |
| Konachan | konachan | 10 | No | Public adapter. |
| Yandere | yandere | 11 | No | Public adapter. |
| CivitAI | civitai | 12 | No | Uses canonical image page URLs for post links. |

## All-sites Mode Selection Rules

When --site is omitted:

- Rule34 is included only if both RULE34_API_KEY and RULE34_USER_ID are present.
- Gelbooru is included only if both GELBOORU_API_KEY and GELBOORU_USER_ID are present.
- Partial credential pairs for either site produce an error.
- All other sites are included by default.

## Post URL Resolution

Each site either:

- uses a site-specific URL template based on post id, or
- sets canonical_post_url directly in the adapter.

CivitAI currently resolves post URLs as:

- https://civitai.com/images/<id>

## Validation Expectations for Site Adapters

Site adapters should provide enough metadata for preflight filtering to work well:

- id must be stable and increasing (or at least comparable against checkpoint logic)
- thumbnail_url should be non-empty for downloadable content
- width and height should be provided when available
- rating should be mapped to a stable representation when possible
- tags should include meaningful searchable text

## Site Adapter Contract

All adapters implement the shared trait with two async operations:

- fetch_recent(last_id)
  - returns new posts above checkpoint
- download_thumbnail(url)
  - downloads bytes for thumbnail image validation + embedding

This keeps ingest generic while allowing each site module to customize API calls, parsing, and retry behavior.
