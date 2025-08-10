# Stage: Base OS
FROM debian:bookworm-slim

# Metadata
LABEL maintainer="MohammadReza"
LABEL description="Postfix mail server with Dovecot SASL & DKIM support"
LABEL version="1.0"

# Prevent interactive tzdata prompt
ARG DEBIAN_FRONTEND=noninteractive
ARG TZ=UTC
ENV TZ=${TZ}

# Install required packages
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       postfix \
       rsyslog \
       dovecot-core dovecot-pop3d dovecot-imapd \
       opendkim opendkim-tools \
       libsasl2-modules \
       openssl \
       bash \
       ca-certificates \
       supervisor \
       tzdata \
    && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime \
    && dpkg-reconfigure -f noninteractive tzdata \
    && rm -rf /var/lib/apt/lists/*

# Create directories
RUN mkdir -p \
      /etc/postfix /var/spool/postfix /var/lib/postfix /var/log/mail \
      /etc/dovecot /var/lib/dovecot \
      /etc/ssl/local \
      /templates/postfix /templates/dovecot && \
    chown -R postfix:postfix /var/spool/postfix /var/lib/postfix && \
    chown -R dovecot:dovecot /var/lib/dovecot
# Note: syslog:adm chown handled at runtime in entrypoint

# Copy Postfix config templates
COPY config/main.cf           /templates/postfix/main.cf.tmpl
COPY config/master.cf         /templates/postfix/master.cf.tmpl
COPY config/virtual           /templates/postfix/virtual.tmpl
COPY config/virtual_domains   /templates/postfix/virtual_domains.tmpl

# Copy entrypoint
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

EXPOSE 25 465 587

VOLUME ["/etc/postfix", "/var/spool/postfix", "/var/lib/postfix", "/var/log/mail", "/etc/ssl/local", "/etc/dovecot", "/var/lib/dovecot"]

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]