# Dockerfile for Postfix + OpenDKIM on Debian Bookworm
FROM debian:bookworm-slim

LABEL maintainer="Mohammad R. Tayyebi <m@tyyi.net>"

# avoid interactive prompts
ARG DEBIAN_FRONTEND=noninteractive

# Install required packages
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       postfix \
       postfix-pcre \
       postfix-mysql \
       libsasl2-modules \
       opendkim \
       bash \
       openssl \
       swaks \
       supervisor \
    && rm -rf /var/lib/apt/lists/*

# Create postfix user/group if not existing
RUN groupadd --system postfix \
    && useradd  --system --no-create-home --gid postfix postfix

# Copy configuration and scripts
COPY config/ /etc/postfix/
COPY scripts/ /scripts/

# Make entrypoint scripts executable
RUN chmod +x /scripts/*.sh

# Ensure spool directory exists and is owned by postfix
RUN mkdir -p /var/spool/postfix \
    && chown -R postfix:postfix /var/spool/postfix

VOLUME ["/etc/postfix", "/var/spool/postfix", "/etc/ssl/private"]

EXPOSE 25 587

CMD ["/scripts/entrypoint.sh"]