FROM rust:alpine AS builder
RUN apk add --update musl-dev pkgconf
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release 2>/dev/null; rm -rf src
COPY src/ src/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    touch src/main.rs && cargo build --release \
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
COPY static/ /app/static/
COPY entrypoint.sh /entrypoint.sh
COPY supervisord.conf /etc/supervisord.conf
RUN chmod +x /entrypoint.sh \
    && mkdir -p /data/ssl /data/dkim /data/mail /data/db /var/spool/postfix \
    && id vmail >/dev/null 2>&1 || adduser -D -s /sbin/nologin vmail \
    && postconf compatibility_level=3.6
EXPOSE 25 587 465 143 993 110 995 8080
VOLUME ["/data"]
ENTRYPOINT ["/entrypoint.sh"]
