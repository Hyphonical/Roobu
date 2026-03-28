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

# Development build with debug symbols and no optimizations
dev: fmt
    cargo build

# Release build with all optimizations
release: fmt
    cargo build --release --locked

# Clean all build artifacts
clean:
    cargo clean

# Run cargo check
check: fmt
    cargo check

# Run clippy linter
lint: fmt
    cargo clippy --all-targets --all-features

# Run tests
test: fmt
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
