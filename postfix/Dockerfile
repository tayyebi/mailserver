FROM alpine:3.19

LABEL maintainer="Mohammad R. Tayyebi <m@tyyi.net>"

RUN apk add --no-cache \
    postfix \
    postfix-pcre \
    postfix-mysql \
    cyrus-sasl \
    cyrus-sasl-plain \
    opendkim \
    bash \
    openssl \
    swaks \
    supervisor

# Create postfix user/group if not existing
RUN addgroup -S postfix && adduser -S postfix -G postfix

COPY config/ /etc/postfix/
COPY scripts/ /scripts/

RUN chmod +x /scripts/*.sh

# Ensure spool directory exists
RUN mkdir -p /var/spool/postfix && chown -R postfix:postfix /var/spool/postfix

VOLUME ["/etc/postfix", "/var/spool/postfix", "/etc/ssl/private"]

EXPOSE 25 587

CMD ["/scripts/entrypoint.sh"]