# Multi-stage build for Rust
FROM rust:1.75-bookworm as builder

WORKDIR /app

# Copy Cargo files
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies
RUN cargo build --release && rm -rf src

# Copy source code
COPY src ./src

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates \
      openssl \
      curl \
      tzdata \
    && rm -rf /var/lib/apt/lists/*

# Create pixelserver user
RUN useradd -r -s /usr/sbin/nologin pixelserver \
    && mkdir -p /opt/pixelserver /data/pixel \
    && chown -R pixelserver:pixelserver /opt/pixelserver /data/pixel

WORKDIR /opt/pixelserver

# Copy the binary from builder stage
COPY --from=builder /app/target/release/pixelserver /usr/local/bin/pixelserver

RUN chmod +x /usr/local/bin/pixelserver

# Set default environment variables
ENV DATA_DIR="/data/pixel"
ENV PIXEL_TLS_CERT="/etc/ssl/certs/server.pem"
ENV PIXEL_TLS_KEY="/etc/ssl/private/server.key"
ENV BIND_ADDRESS="0.0.0.0:8443"
ENV LOG_LEVEL="info"
ENV DEV_MODE="false"

EXPOSE 8443

USER pixelserver

CMD ["/usr/local/bin/pixelserver"]
