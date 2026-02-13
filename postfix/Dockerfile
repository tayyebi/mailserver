# Build reinject from additional context
FROM rust:1-alpine3.21 AS reinject-builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY --from=reinject Cargo.toml ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src
COPY --from=reinject src ./src
RUN cargo build --release

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
        cyrus-sasl \
        cyrus-sasl-login \
        openssl \
        bash \
        ca-certificates \
        tzdata \
        gettext \
        mailx \
        netcat-openbsd \
    && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime

# Create only directories Postfix needs
RUN mkdir -p /var/spool/postfix/{active,bounce,corrupt,defer,deferred,flush,hold,incoming,maildrop,pid,private,public,saved,trace} \
    /var/log/postfix \
    /templates

# Ensure postfix ownership
RUN chown -R postfix:postfix /var/spool/postfix /var/log/postfix

# Apply Postfix permissions
RUN postfix set-permissions || true

# Sanity check
RUN postfix check

# Ensure postfix is in postdrop group
RUN addgroup postfix postdrop 2>/dev/null || true

# Copy reinjection binary
COPY --from=reinject-builder /app/target/release/postfix-reinject /usr/local/bin/postfix-reinject
RUN chmod +x /usr/local/bin/postfix-reinject
