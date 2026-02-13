FROM alpine:3.21

LABEL maintainer="Mohammad R. Tayyebi <m@tyyi.net>"
LABEL description="Dovecot IMAP/LMTP service"
LABEL version="1.1"

ARG TZ=UTC
ENV TZ=${TZ}

RUN apk add --no-cache \
        dovecot \
        dovecot-lmtpd \
        dovecot-pop3d \
        tzdata \
        openssl \
    && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime

RUN mkdir -p /etc/dovecot /var/lib/dovecot /mail \
    && chown -R root:root /etc/dovecot \
    && chown -R dovecot:dovecot /var/lib/dovecot /mail

# Ensure passwd file exists if not mounted
RUN touch /etc/dovecot/passwd && chown dovecot:dovecot /etc/dovecot/passwd && chmod 640 /etc/dovecot/passwd
