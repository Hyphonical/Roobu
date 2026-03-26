# Roobu build orchestration
# Install `just` via: cargo install just

# Use PowerShell on Windows, sh on Unix
set windows-shell := ["pwsh.exe", "-NoLogo", "-Command"]
set shell := ["bash", "-uc"]

# Default recipe - build everything
default: dev

# Development build with debug symbols and no optimizations
dev:
    cargo build

# Release build with all optimizations
release:
    cargo build --release --locked

# Clean all build artifacts
clean:
    cargo clean

# Run cargo check
check:
    cargo check

# Run clippy linter
lint:
    cargo clippy --all-targets --all-features

# Format code
fmt:
    cargo fmt

# Run tests
test:
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
