# Stage: Base OS
FROM debian:bookworm-slim

# Metadata
LABEL maintainer="MohammadReza"
LABEL description="Dovecot IMAP/POP3 service"
LABEL version="1.0"

# Prevent interactive tzdata prompt
ARG DEBIAN_FRONTEND=noninteractive
ARG TZ=UTC
ENV TZ=${TZ}

# Install Dovecot and timezone data
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       dovecot-core \
       dovecot-imapd \
       dovecot-lmtpd \
       tzdata \
    && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime \
    && dpkg-reconfigure -f noninteractive tzdata \
    && rm -rf /var/lib/apt/lists/*

# Create data directories
RUN mkdir -p /etc/dovecot /var/lib/dovecot \
    && chown -R root:root /etc/dovecot \
    && chown -R dovecot:dovecot /var/lib/dovecot

VOLUME ["/etc/dovecot", "/var/lib/dovecot", "/mail"]

EXPOSE 143 993

CMD ["dovecot", "-F"]