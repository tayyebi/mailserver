# Build stage
FROM rust:1-alpine3.21 AS builder
RUN apk add --no-cache musl-dev openssl-dev pkgconf
WORKDIR /app
COPY Cargo.toml ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && rm -rf src
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && cp target/release/postfix-reinject /tmp/postfix-reinject

# Runtime stage — tiny image that just holds the binary
FROM alpine:3.21
COPY --from=builder /tmp/postfix-reinject /usr/local/bin/postfix-reinject
RUN chmod +x /usr/local/bin/postfix-reinject
