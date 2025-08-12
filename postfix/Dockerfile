FROM debian:bookworm

LABEL maintainer="Mohammad R. Tayyebi <m@tyyi.net>"
LABEL description="Postfix mail server with Dovecot SASL & DKIM support"
LABEL version="1.1"

ARG DEBIAN_FRONTEND=noninteractive
ARG TZ=UTC
ENV TZ=${TZ}

# Install required packages
RUN apt-get update \
    && apt-get install -y \
        postfix \
        libsasl2-modules \
        openssl \
        bash \
        ca-certificates \
        tzdata \
        swaks \
        gettext \
        man-db \
        postfix-doc \
	&& ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime \
	&& dpkg-reconfigure -f noninteractive tzdata

# Create directories
RUN mkdir -p /var/spool/postfix/active /var/spool/postfix/bounce /var/spool/postfix/corrupt \
        /var/spool/postfix/defer /var/spool/postfix/deferred /var/spool/postfix/flush \
        /var/spool/postfix/hold /var/spool/postfix/incoming /var/spool/postfix/private \
        /var/spool/postfix/maildrop /var/spool/postfix/pid /var/spool/postfix/public \
        /var/spool/postfix/saved /var/spool/postfix/trace \
        && chown -R postfix:postfix /var/spool/postfix \
        && postfix set-permissions

RUN postfix check


# Copy Postfix config templates
COPY config/main.cf.tmpl			/templates/postfix/main.cf.tmpl
COPY config/master.cf.tmpl			/templates/postfix/master.cf.tmpl
COPY config/virtual_aliases.tmpl	/templates/postfix/virtual_aliases.tmpl
COPY config/virtual_domains.tmpl	/templates/postfix/virtual_domains.tmpl
COPY config/vmailbox.tmpl			/templates/postfix/vmailbox.tmpl

# Add users and groups
RUN if ! id -nG postfix | grep -qw postdrop; then \
        echo "Adding postfix to postdrop group"; \
        usermod -aG postdrop postfix; \
    fi

# Copy entrypoint
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

EXPOSE 25 587

VOLUME ["/etc/postfix", "/var/spool/postfix", "/var/lib/postfix"]

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]