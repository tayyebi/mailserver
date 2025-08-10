FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    opendkim opendkim-tools tzdata ca-certificates tini \
  && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /etc/opendkim/keys /var/run/opendkim /var/log/mail \
 && chown -R opendkim:opendkim /etc/opendkim /var/run/opendkim /var/log/mail

COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

USER opendkim

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/usr/local/bin/entrypoint.sh"]