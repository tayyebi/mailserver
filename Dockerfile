FROM rust:alpine AS builder
RUN apk add --update musl-dev pkgconf
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release 2>/dev/null; rm -rf src
COPY templates/ templates/
COPY src/ src/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release \
    && cp target/release/mailserver /output \
    && strip /output

FROM alpine:3.21
RUN --mount=type=cache,target=/var/cache/apk \
    apk add --update \
    postfix \
    dovecot \
    dovecot-lmtpd \
    dovecot-pop3d \
    opendkim \
    opendkim-utils \
    supervisor \
    openssl
COPY --from=builder /output /usr/local/bin/mailserver
COPY templates/config/ /app/templates/config/
COPY migrations/ /app/migrations/
COPY static/ /app/static/
COPY entrypoint.sh /entrypoint.sh
COPY supervisord.conf /etc/supervisord.conf
RUN chmod +x /entrypoint.sh \
    && mkdir -p /data/ssl /data/dkim /data/mail /data/db /var/spool/postfix \
    && addgroup -S vmail 2>/dev/null; adduser -S -D -H -G vmail -s /sbin/nologin vmail 2>/dev/null; \
    postconf compatibility_level=3.6
EXPOSE 25 587 465 2525 143 993 110 995 8080
VOLUME ["/data"]
ENTRYPOINT ["/entrypoint.sh"]
