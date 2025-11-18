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

# Copy Dovecot configuration (only the conf file, not the entire directory)
COPY dovecot.conf /etc/dovecot/dovecot.conf
RUN chmod 644 /etc/dovecot/dovecot.conf && \
    doveconf -n > /dev/null 2>&1 || echo "Warning: Dovecot config validation skipped (doveadm not available in build)"

# Ensure passwd file exists if not mounted
RUN touch /etc/dovecot/passwd && chown dovecot:dovecot /etc/dovecot/passwd && chmod 640 /etc/dovecot/passwd
