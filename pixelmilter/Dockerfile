# Multi-stage build for Rust
FROM rust:1.82-bookworm as builder

WORKDIR /app

# Copy Cargo files
COPY Cargo.toml ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies with cache mount
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release && rm -rf src

# Copy source code
COPY src ./src

# Build the application with cache mount
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp /app/target/release/pixelmilter /tmp/pixelmilter

# Runtime stage
FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates \
      tzdata \
    && rm -rf /var/lib/apt/lists/*

# Create pixel user
RUN useradd -r -s /usr/sbin/nologin pixel \
    && mkdir -p /opt/pixelmilter /data/pixel /var/run/pixelmilter \
    && chown -R pixel:pixel /opt/pixelmilter /data/pixel /var/run/pixelmilter

WORKDIR /opt/pixelmilter

# Copy the binary from builder stage
COPY --from=builder /tmp/pixelmilter /usr/local/bin/pixelmilter

# Copy entrypoint script
COPY pixelmilter-entrypoint.sh /usr/local/bin/pixelmilter-entrypoint.sh

RUN chmod +x /usr/local/bin/pixelmilter /usr/local/bin/pixelmilter-entrypoint.sh

# Set default environment variables
ENV PIXEL_BASE_URL="https://localhost:8443/pixel?id="
ENV REQUIRE_OPT_IN="false"
ENV OPT_IN_HEADER="X-Track-Open"
ENV DISCLOSURE_HEADER="X-Tracking-Notice"
ENV INJECT_DISCLOSURE="true"
ENV DATA_DIR="/data/pixel"
ENV LOG_LEVEL="info"
ENV PIXEL_MILTER_SOCKET="/var/run/pixelmilter/pixel.sock"

ENTRYPOINT ["/usr/local/bin/pixelmilter-entrypoint.sh"]
CMD []
