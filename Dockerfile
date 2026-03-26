FROM rust:1.88-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release && \
    find target -name "libonnxruntime.so*" -type f \
      -exec cp {} /usr/local/lib/ \; && \
    ldconfig

FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/lib/libonnxruntime* /usr/local/lib/
COPY --from=builder /build/target/release/roobu /app/roobu
RUN ldconfig
ENTRYPOINT ["/app/roobu"]