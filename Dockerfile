# ── Build Stage ──────────────────────────────────────────────────────────────
FROM rust:1.88-trixie AS builder
WORKDIR /build

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src

# Copy actual source and rebuild
COPY src ./src
RUN touch src/main.rs && cargo build --release

# Extract ONNX Runtime shared libraries
RUN find target -name "libonnxruntime.so*" -type f \
    -exec cp {} /usr/local/lib/ \; && \
    ldconfig

# ── Runtime Stage ────────────────────────────────────────────────────────────
FROM debian:trixie-slim

# Install minimal runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy ONNX Runtime libraries from builder
COPY --from=builder /usr/local/lib/libonnxruntime* /usr/local/lib/
RUN ldconfig

# Copy the compiled binary
COPY --from=builder /build/target/release/roobu /app/roobu

WORKDIR /app

# Create data directory for checkpoint persistence
RUN mkdir -p /app/data

ENTRYPOINT ["/app/roobu"]