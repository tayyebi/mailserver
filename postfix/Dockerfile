# Build stage for postfix-reinject
FROM rust:1-alpine3.21 AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY reinject/ .
RUN cargo build --release

# Final stage
FROM alpine:3.21

LABEL maintainer="Mohammad R. Tayyebi <m@tyyi.net>"
LABEL description="Postfix mail server with Dovecot SASL & DKIM support"
LABEL version="1.1"

ARG TZ=UTC
ENV TZ=${TZ}

# Install required packages
RUN apk add --no-cache \
        postfix \
        postfix-pcre \
        cyrus-sasl-login \
        cyrus-sasl-plain \
        openssl \
        bash \
        ca-certificates \
        tzdata \
        gettext \
        mailx \
        netcat-openbsd \
    && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime

# Create directories
RUN mkdir -p /var/spool/postfix/active /var/spool/postfix/bounce /var/spool/postfix/corrupt \
        /var/spool/postfix/defer /var/spool/postfix/deferred /var/spool/postfix/flush \
        /var/spool/postfix/hold /var/spool/postfix/incoming /var/spool/postfix/private \
        /var/spool/postfix/maildrop /var/spool/postfix/pid /var/spool/postfix/public \
        /var/spool/postfix/saved /var/spool/postfix/trace \
        /var/log/postfix
RUN postfix set-permissions
RUN postfix check

# Create templates directory (templates will be mounted via volumes)
RUN mkdir -p /templates

# Ensure postfix is in postdrop group
RUN addgroup postfix postdrop 2>/dev/null || true

# Copy reinjection binary from builder
COPY --from=builder /app/target/release/postfix-reinject /usr/local/bin/postfix-reinject
RUN chmod +x /usr/local/bin/postfix-reinject || true

RUN mkdir -p /usr/local/bin
