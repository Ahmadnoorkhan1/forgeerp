# Multi-stage Dockerfile for local development builds.
# Builds the `forgeerp-api` workspace member and ships a small runtime image.

FROM rust:1-bookworm AS builder
WORKDIR /app

# Build dependencies first for better caching.
COPY Cargo.toml Cargo.lock ./
COPY crates/core/Cargo.toml crates/core/Cargo.toml
COPY crates/events/Cargo.toml crates/events/Cargo.toml
COPY crates/auth/Cargo.toml crates/auth/Cargo.toml
COPY crates/infra/Cargo.toml crates/infra/Cargo.toml
COPY crates/observability/Cargo.toml crates/observability/Cargo.toml
COPY crates/api/Cargo.toml crates/api/Cargo.toml

# Dummy sources to compile dependency graph.
RUN mkdir -p crates/api/src crates/core/src crates/events/src crates/auth/src crates/infra/src crates/observability/src \
  && printf 'fn main(){}' > crates/api/src/main.rs \
  && printf '' > crates/api/src/lib.rs \
  && printf '' > crates/core/src/lib.rs \
  && printf '' > crates/events/src/lib.rs \
  && printf '' > crates/auth/src/lib.rs \
  && printf '' > crates/infra/src/lib.rs \
  && printf '' > crates/observability/src/lib.rs \
  && cargo build -p forgeerp-api --release

# Now copy the real sources and build.
COPY . .
RUN cargo build -p forgeerp-api --release

FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/forgeerp-api /app/forgeerp-api

EXPOSE 8080
CMD ["/app/forgeerp-api"]


