# Frontend Handoff + Interface Sketch

This document is the practical handoff from backend to future frontend work.

## Goal

Build a first useful website without backend churn:

- text search
- image upload search (drag/drop + file picker)
- similar image search from a result card
- site filtering with no hardcoded site names
- masonry-style results gallery

## Backend Contract Workflow

Use this every time backend API changes.

1. Export frozen OpenAPI snapshot:
   - roobu contract export --output docs/api/openapi.v1.json
2. Regenerate TypeScript API schema:
   - npx --yes openapi-typescript@7.10.1 docs/api/openapi.v1.json -o docs/frontend/roobu-api-client.ts
3. Validate no contract drift:
   - roobu contract check --snapshot docs/api/openapi.v1.json
4. Run backend checks:
   - cargo check
   - cargo test
   - cargo clippy --all-targets --all-features -- -D warnings

## Frontend Data Sources (No Hardcoded Site Values)

Use API data for anything that can change over time.

| UI Piece | Source API | Notes |
| --- | --- | --- |
| Site selector options | GET /api/sites | Derive selector options from data[].name. Never hardcode site list in frontend. |
| Search results (text/url) | GET /api/search | For text or remote image_url queries. |
| Search results (upload) | POST /api/search/upload | For drag/drop or file picker image upload. |
| Similar button on card | GET /api/search/similar/{site}/{post_id} | Opens related content from clicked card. |
| Landing feed | GET /api/recent | Good default when no query entered yet. |
| Header status badge | GET /api/ingest/status + WS /api/ws/ingest | Poll once, then stream updates. |
| Activity chart | GET /api/activity | Optional for dashboard page.

## Rough Interface (Desktop)

Use this as a starting layout, not a strict pixel spec.

```text
+--------------------------------------------------------------------------------+
| Roobu logo               [Search input..............................] [Search] |
|                         [Drag/drop zone + file picker] [Site multi-select v]   |
|                         [Limit] [Image weight slider] [Clear]                  |
+--------------------------------------------------------------------------------+
| Status: ingest running / idle      Sites indexed: N      Last update: ...      |
+--------------------------------------------------------------------------------+
| Results (masonry gallery)                                                   ^   |
| +-----------------+ +---------------------+ +---------------+               |   |
| | image preview   | | image preview       | | image preview |               |   |
| | site + id       | | site + id           | | site + id     |               |   |
| | score + size    | | score + size        | | score + size  |               |   |
| | [Open] [Similar]| | [Open] [Similar]    | | [Open][Similar]|              |   |
| +-----------------+ +---------------------+ +---------------+               |   |
| ... infinite scroll using meta.next_offset ...                              |   |
+--------------------------------------------------------------------------------+
```

## Rough Interface (Mobile)

```text
+--------------------------------------+
| Roobu                                |
| [Search input.....................]  |
| [Drag/drop zone / Upload button]     |
| [Site selector] [Limit] [Weight]     |
| [Search] [Clear]                     |
+--------------------------------------+
| Status row                           |
+--------------------------------------+
| Masonry cards in 2 columns           |
| [card] [card]                        |
| [card] [card]                        |
+--------------------------------------+
```

## Component Checklist

- Search bar (q)
- Drag/drop zone + file picker (image)
- Site selector (multi-select)
- Limit input
- Image weight control
- Search and clear actions
- Masonry gallery with image cards
- Card actions: Open source, Similar
- Infinite scroll or Load more
- Ingest status badge (REST + websocket)

## Behavior Rules (Avoid Hardcoded Values)

- Do not hardcode sites in frontend code.
- Build site selector from GET /api/sites response each load.
- Use meta.next_offset for pagination; do not assume page counts.
- Respect server clamping for limit/image_weight.
- Keep endpoint paths centralized in one client wrapper (docs/frontend/roobu-api-runtime.ts pattern).

## Suggested Search Behavior

1. If user selected upload image:
   - call POST /api/search/upload
2. Else if user entered text or image_url:
   - call GET /api/search
3. Else:
   - fetch GET /api/recent for default gallery

## Result Card Fields

Render from PostDto data:

- thumbnail (thumbnail_url)
- optional full image link (direct_image_url)
- site + post_id
- score if present
- resolution (width x height)
- optional tags/rating
- source link (post_url)

## Error and Empty States

- Empty search: show recent feed prompt.
- No results: show friendly message + clear filters button.
- Upload too large/invalid: display API error text.
- Site list unavailable: disable selector and continue with all sites.

## Practical First Build Sequence

1. Implement page shell and typed API client wiring.
2. Load site selector options from GET /api/sites.
3. Implement text search flow.
4. Add drag/drop upload flow.
5. Render masonry gallery cards.
6. Add Similar action per card.
7. Add ingest status indicator.
8. Add pagination/infinite scroll.
