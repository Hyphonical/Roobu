# Frontend MVP Checklist

This checklist is for the handoff point where backend work is considered frontend-ready.

## Contract Artifacts (already in repo)

- OpenAPI snapshot: docs/api/openapi.v1.json
- Generated TypeScript schema: docs/frontend/roobu-api-client.ts
- Typed runtime wrapper: docs/frontend/roobu-api-runtime.ts

## Backend Readiness Checklist

- [ ] Run contract drift check:
  - roobu contract check --snapshot docs/api/openapi.v1.json
- [ ] Run backend quality gates:
  - cargo fmt --all
  - cargo check
  - cargo test
  - cargo clippy --all-targets --all-features -- -D warnings
- [ ] Confirm API docs are reachable in serve mode:
  - GET /api/openapi.json
  - GET /swagger-ui/
- [ ] Confirm ingest websocket is reachable:
  - WS /api/ws/ingest

## MVP API Surface Checklist

Search and retrieval:

- [ ] GET /api/search
- [ ] POST /api/search/upload (multipart image upload)
- [ ] GET /api/search/similar/{site}/{post_id}
- [ ] GET /api/post/{site}/{post_id}
- [ ] GET /api/recent

Monitoring and site metadata:

- [ ] GET /api/activity
- [ ] GET /api/sites
- [ ] GET /api/ingest/status
- [ ] WS /api/ws/ingest

## Frontend Input Contracts to Rely On

GET /api/search query params:

- q: optional string
- image_url: optional string
- site: optional repeated values or comma-separated
- limit: optional number (server clamps to 1..100)
- image_weight: optional number (server clamps to 0.0..1.0)

POST /api/search/upload multipart fields:

- image: optional binary file (required if q is missing)
- q: optional string (required if image is missing)
- site: optional repeated values or comma-separated
- limit: optional number (server clamps to 1..100)
- image_weight: optional number (server clamps to 0.0..1.0)

Response envelope:

- Every API response uses data + optional meta
- List endpoints include meta.count and optional meta.next_offset

## WebSocket Event Checklist

- [ ] Handle connected event
- [ ] Handle lagged event
- [ ] Handle ingest events:
  - StatusSnapshot
  - CycleComplete
  - CycleFailed
  - CheckpointUpdated
  - Sleeping

Use IngestWsEvent from docs/frontend/roobu-api-runtime.ts for event typing.

## VPS / Docker Deployment Checklist

- [ ] Expose Roobu API port from container
- [ ] If frontend is separate static app, allow CORS or serve behind same reverse proxy
- [ ] Ensure reverse proxy forwards websocket upgrades for /api/ws/ingest
- [ ] Keep model files mounted and checkpoint path persisted
- [ ] Keep Qdrant data volume persistent

## When Backend Changes Later

If backend API changes intentionally:

1. Regenerate contract snapshot:
   - roobu contract export --output docs/api/openapi.v1.json
2. Regenerate TypeScript schema:
   - npx --yes openapi-typescript@7.10.1 docs/api/openapi.v1.json -o docs/frontend/roobu-api-client.ts
3. Update runtime wrapper if endpoint signatures changed:
   - docs/frontend/roobu-api-runtime.ts
4. Re-run checks:
   - roobu contract check --snapshot docs/api/openapi.v1.json
   - cargo test
