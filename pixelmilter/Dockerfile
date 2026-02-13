# Build stage
FROM rust:1-alpine3.21 AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && rm -rf src
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && cp target/release/pixelmilter /tmp/pixelmilter

# Runtime stage
FROM alpine:3.21
RUN apk add --no-cache ca-certificates tzdata
RUN adduser -D -s /sbin/nologin pixel \
    && mkdir -p /opt/pixelmilter /data/pixel \
    && chown -R pixel:pixel /opt/pixelmilter /data/pixel
WORKDIR /opt/pixelmilter
COPY --from=builder /tmp/pixelmilter /usr/local/bin/pixelmilter
RUN chmod +x /usr/local/bin/pixelmilter

ENV PIXEL_BASE_URL="https://localhost:8443/pixel?id="
ENV TRACKING_REQUIRES_OPT_IN="false"
ENV OPT_IN_HEADER="X-Track-Open"
ENV DISCLOSURE_HEADER="X-Tracking-Notice"
ENV INJECT_DISCLOSURE="true"
ENV DATA_DIR="/data/pixel"
ENV LOG_LEVEL="info"
ENV PIXEL_MILTER_ADDRESS="0.0.0.0:8892"

ENTRYPOINT ["/usr/local/bin/pixelmilter-entrypoint.sh"]
CMD []
