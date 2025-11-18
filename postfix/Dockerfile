FROM debian:bookworm

LABEL maintainer="Mohammad R. Tayyebi <m@tyyi.net>"
LABEL description="Postfix mail server with Dovecot SASL & DKIM support"
LABEL version="1.1"

ARG DEBIAN_FRONTEND=noninteractive
ARG TZ=UTC
ENV TZ=${TZ}

# Install required packages
RUN apt-get update \
    && apt-get install -y --install-recommends \
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
        mailutils \
	&& ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime \
	&& dpkg-reconfigure -f noninteractive tzdata

# Create directories
RUN mkdir -p /var/spool/postfix/active /var/spool/postfix/bounce /var/spool/postfix/corrupt \
        /var/spool/postfix/defer /var/spool/postfix/deferred /var/spool/postfix/flush \
        /var/spool/postfix/hold /var/spool/postfix/incoming /var/spool/postfix/private \
        /var/spool/postfix/maildrop /var/spool/postfix/pid /var/spool/postfix/public \
        /var/spool/postfix/saved /var/spool/postfix/trace
RUN mkdir -p /usr/share/doc/postfix \
    && touch /usr/share/doc/postfix/MAILLOG_README \
    && touch /usr/share/doc/postfix/TLS_README \
    && touch /usr/share/doc/postfix/SASL_README \
    && touch /usr/share/doc/postfix/LDAP_README \
    && touch /usr/share/doc/postfix/DB_README \
    && touch /usr/share/doc/postfix/CYRUS_README \
    && touch /usr/share/doc/postfix/UUCP_README \
    && touch /usr/share/doc/postfix/QMQP_README \
    && touch /usr/share/doc/postfix/SMTPD_POLICY_README \
    && touch /usr/share/doc/postfix/RESTRICTION_CLASS_README \
    && touch /usr/share/doc/postfix/SMTPUTF8_README \
    && touch /usr/share/doc/postfix/POSTSCREEN_3_5_README
RUN postfix set-permissions
RUN postfix check

# Create templates directory (templates will be mounted via volumes)
RUN mkdir -p /templates

# Add users and groups
RUN if ! id -nG postfix | grep -qw postdrop; then \
        echo "Adding postfix to postdrop group"; \
        usermod -aG postdrop postfix; \
    fi

# Copy HealthCheck Script
COPY healthcheck.sh /usr/local/bin/healthcheck.sh

# Copy entrypoint
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# Copy content filter script (will be replaced by volume mount if available)
COPY pixel-content-filter.sh /usr/local/bin/pixel-content-filter.sh
RUN chmod +x /usr/local/bin/pixel-content-filter.sh || true

# Copy pixelmilter binary from pixelmilter image (if available at build time)
# This will be overridden by volume mount if pixelmilter container is running
RUN mkdir -p /usr/local/bin