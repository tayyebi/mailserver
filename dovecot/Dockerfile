FROM debian:bookworm
LABEL maintainer="Mohammad R. Tayyebi <m@tyyi.net>" description="Dovecot IMAP/LMTP service" version="1.1"

ARG DEBIAN_FRONTEND=noninteractive
ARG TZ=UTC
ENV TZ=${TZ}

RUN apt-get update \
 && apt-get install -y dovecot-core dovecot-imapd dovecot-lmtpd dovecot-pop3d \
 	tzdata openssl \
 && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime \
 && dpkg-reconfigure -f noninteractive tzdata

RUN mkdir -p /etc/dovecot /var/lib/dovecot /mail \
 && chown -R root:root /etc/dovecot \
 && chown -R dovecot:dovecot /var/lib/dovecot /mail

COPY dovecot.conf /etc/dovecot/dovecot.conf
COPY passwd /etc/dovecot/passwd

VOLUME ["/mail"]

EXPOSE 143 993 24 12345
CMD ["dovecot", "-F"]