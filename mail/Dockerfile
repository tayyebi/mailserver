FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    postfix postfix-pcre libsasl2-modules dovecot-core dovecot-sasl \
    ca-certificates openssl rsyslog logrotate tzdata tini \
  && rm -rf /var/lib/apt/lists/*

# Ensure log dirs
RUN mkdir -p /var/log/mail && chown -R syslog:adm /var/log/mail

# Non-root runtime users exist already (postfix, dovecot). We’ll manage permissions on volumes.

COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# Templates mounted read-only at /templates/... by compose
# Persistent configs mounted at /etc/postfix and /etc/dovecot

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/usr/local/bin/entrypoint.sh"]
