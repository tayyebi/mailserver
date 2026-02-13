FROM alpine:3.21

LABEL maintainer="Mohammad R. Tayyebi <m@tyyi.net>"
LABEL description="Postfix mail server with Dovecot SASL & DKIM support"
LABEL version="1.1"

ARG TZ=UTC
ENV TZ=${TZ}

# Install required packages
RUN apk add --no-cache \
        postfix \
        postfix-pcre \
        cyrus-sasl \
        cyrus-sasl-login \
        openssl \
        bash \
        ca-certificates \
        tzdata \
        gettext \
        mailx \
        netcat-openbsd \
    && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime

# Create only directories Postfix needs
RUN mkdir -p /var/spool/postfix/{active,bounce,corrupt,defer,deferred,flush,hold,incoming,maildrop,pid,private,public,saved,trace} \
    /var/spool/postfix/etc \
    /var/log/postfix \
    /templates

# Ensure postfix ownership
RUN chown -R postfix:postfix /var/spool/postfix /var/log/postfix

# Apply Postfix permissions
RUN postfix set-permissions || true

# Sanity check
RUN postfix check

# Ensure postfix is in postdrop group
RUN addgroup postfix postdrop 2>/dev/null || true

# postfix-reinject binary is bind-mounted at runtime from reinject service

# Copy entrypoint and healthcheck scripts
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh
COPY healthcheck.sh /usr/local/bin/healthcheck.sh
RUN chmod +x /usr/local/bin/healthcheck.sh

# Copy config templates
COPY main.cf.tmpl /templates/main.cf.tmpl
COPY master.cf.tmpl /templates/master.cf.tmpl
COPY virtual_aliases.tmpl /templates/virtual_aliases.tmpl
COPY virtual_domains.tmpl /templates/virtual_domains.tmpl
COPY vmailbox.tmpl /templates/vmailbox.tmpl
COPY pixel-content-filter.sh.tmpl /templates/pixel-content-filter.sh.tmpl
COPY resolv.conf /var/spool/postfix/etc/resolv.conf
COPY resolv.conf /etc/resolv.conf
