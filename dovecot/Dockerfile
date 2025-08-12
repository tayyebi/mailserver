FROM debian:bookworm-slim
LABEL maintainer="Mohammad R. Tayyebi <m@tyyi.net>" description="Dovecot IMAP/LMTP service" version="1.1"

ARG DEBIAN_FRONTEND=noninteractive
ARG TZ=UTC
ENV TZ=${TZ}

RUN apt-get update \
 && apt-get install -y --no-install-recommends dovecot-core dovecot-imapd dovecot-lmtpd tzdata openssl \
 && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime \
 && dpkg-reconfigure -f noninteractive tzdata \
 && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /etc/dovecot /var/lib/dovecot /mail \
 && chown -R root:root /etc/dovecot \
 && chown -R dovecot:dovecot /var/lib/dovecot /mail

VOLUME ["/etc/dovecot", "/var/lib/dovecot", "/mail"]

EXPOSE 143 993 24 12345
CMD ["dovecot", "-F"]