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
    cargo build --release && cp target/release/pixelserver /tmp/pixelserver

# Runtime stage
FROM alpine:3.21
RUN apk add --no-cache bash ca-certificates openssl curl tzdata
RUN adduser -D -s /sbin/nologin pixelserver \
    && mkdir -p /opt/pixelserver \
    && chown -R pixelserver:pixelserver /opt/pixelserver
WORKDIR /opt/pixelserver
COPY --from=builder /tmp/pixelserver /usr/local/bin/pixelserver
RUN chmod +x /usr/local/bin/pixelserver

COPY pixelserver-entrypoint.sh /usr/local/bin/pixelserver-entrypoint.sh
RUN chmod +x /usr/local/bin/pixelserver-entrypoint.sh

ENV DATA_DIR="/data/pixel"
ENV PIXEL_TLS_CERT="/etc/ssl/certs/server.pem"
ENV PIXEL_TLS_KEY="/etc/ssl/private/server.key"
ENV BIND_ADDRESS="0.0.0.0:8443"
ENV LOG_LEVEL="info"
ENV DEV_MODE="false"

EXPOSE 8443

ENTRYPOINT ["/usr/local/bin/pixelserver-entrypoint.sh"]
CMD []
