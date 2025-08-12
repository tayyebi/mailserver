FROM debian:bookworm-slim

LABEL maintainer="Mohammad R. Tayyebi <m@tyyi.net>"
LABEL description="Postfix mail server with Dovecot SASL & DKIM support"
LABEL version="1.1"

ARG DEBIAN_FRONTEND=noninteractive
ARG TZ=UTC
ENV TZ=${TZ}

# Install required packages
RUN apt-get update \
	&& apt-get install -y --no-install-recommends \
		postfix \
		libsasl2-modules \
		openssl \
		bash \
		ca-certificates \
		tzdata \
		swaks \
		gettext \
	&& ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime \
	&& dpkg-reconfigure -f noninteractive tzdata \
	&& rm -rf /var/lib/apt/lists/*

# Create directories
RUN mkdir -p \
		/etc/postfix /var/spool/postfix /var/lib/postfix /var/log/mail \
		/templates/postfix \
	&& chown -R postfix:postfix /var/spool/postfix /var/lib/postfix

# Copy Postfix config templates
COPY config/main.cf.tmpl			/templates/postfix/main.cf.tmpl
COPY config/master.cf.tmpl			/templates/postfix/master.cf.tmpl
COPY config/virtual_aliases.tmpl	/templates/postfix/virtual_aliases.tmpl
COPY config/virtual_domains.tmpl	/templates/postfix/virtual_domains.tmpl
COPY config/vmailbox.tmpl			/templates/postfix/vmailbox.tmpl

# Copy entrypoint
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

EXPOSE 25 587

VOLUME ["/etc/postfix", "/var/spool/postfix", "/var/lib/postfix"]

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]