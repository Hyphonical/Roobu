# Roobu build orchestration
# Install `just` via: cargo install just

# Use PowerShell on Windows, sh on Unix
set windows-shell := ["pwsh.exe", "-NoLogo", "-Command"]
set shell := ["bash", "-uc"]

# Default recipe - build everything
default: dev

# Verify formatting before build-related tasks
fmt:
    cargo fmt --all --check

# Development build with debug symbols and no optimizations, also run the linter
dev: fmt lint
    cargo build

# Release build with all optimizations
release: fmt lint
    cargo build --release --locked

# Clean all build artifacts
clean:
    cargo clean

# Run cargo check
check: fmt lint
    cargo check

# Export frozen OpenAPI contract snapshot
contract-export:
    cargo run -- contract export --output docs/api/openapi.v1.json

# Validate generated OpenAPI against frozen snapshot
contract-check:
    cargo run -- contract check --snapshot docs/api/openapi.v1.json

# Generate typed TypeScript API schema from frozen OpenAPI snapshot
frontend-types:
    npx --yes openapi-typescript@7.10.1 docs/api/openapi.v1.json -o docs/frontend/roobu-api-client.ts

# Run clippy linter
lint: fmt
    cargo clippy --all-targets --all-features -- -D warnings

# Run tests
test: fmt lint
    cargo test

# Build the local Docker image
docker-build:
    docker build -t roobu:latest .

# Start Qdrant + roobu with docker compose
docker-up:
    docker compose up --build -d

# Stop all compose services
docker-down:
    docker compose down

# Remove compose services and all attached volumes (full DB reset)
docker-reset:
    docker compose down -v
