# Dockerfile for Postfix 
FROM debian:bookworm-slim

LABEL maintainer="m@tyyi.net" \
      description="Postfix mail server container with Dovecot SASL & DKIM support"

ENV DEBIAN_FRONTEND=noninteractive

# Install required packages
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      postfix \
      rsyslog \
      dovecot-core dovecot-sasl \
      dovecot-pop3d dovecot-imapd \
      opendkim opendkim-tools \
      libsasl2-modules \
      openssl \
      bash \
      ca-certificates \
      supervisor \
    && rm -rf /var/lib/apt/lists/*

# Create directories for configs, SSL, spool, logs
RUN mkdir -p \
      /etc/postfix /var/spool/postfix /var/lib/postfix /var/log/mail \
      /etc/dovecot /var/lib/dovecot \
      /etc/ssl/local \
      /templates/postfix /templates/dovecot \
    && chown -R postfix:postfix /var/spool/postfix /var/lib/postfix \
    && chown -R dovecot:dovecot /var/lib/dovecot \
    && chown -R syslog:adm /var/log/mail

# Copy Postfix config templates (used for first-run seeding)
COPY config/main.cf      /templates/postfix/main.cf.tmpl
COPY config/master.cf    /templates/postfix/master.cf.tmpl
COPY config/virtual      /templates/postfix/virtual.tmpl
COPY config/virtual_domains /templates/postfix/virtual_domains.tmpl

# If you have Dovecot config templates for seeding, copy here:
# COPY ../dovecot/config/ /templates/dovecot/

# Copy entrypoint script
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# Expose mail ports
EXPOSE 25 465 587

# Volumes for persistence & configs
VOLUME ["/etc/postfix", "/var/spool/postfix", "/var/lib/postfix", "/var/log/mail", "/etc/ssl/local", "/etc/dovecot", "/var/lib/dovecot"]

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
