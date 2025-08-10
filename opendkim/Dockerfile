# Stage: Base OS
FROM debian:bookworm-slim

# Metadata
LABEL maintainer="MohammadReza"
LABEL description="OpenDKIM signing service"
LABEL version="1.0"

# Prevent interactive tzdata prompt
ARG DEBIAN_FRONTEND=noninteractive
ARG TZ=UTC
ENV TZ=${TZ}

# Install packages
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       opendkim \
       tzdata \
    && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime \
    && dpkg-reconfigure -f noninteractive tzdata \
    && rm -rf /var/lib/apt/lists/*

# Create and secure key directory
RUN mkdir -p /etc/opendkim/keys \
    && chown -R opendkim:opendkim /etc/opendkim

VOLUME [ "/etc/opendkim/conf", "/etc/opendkim/keys" ]

# Expose control port if you use it (optional)
EXPOSE 8891

# Run in foreground with specified config
ENTRYPOINT [ "opendkim", "-f", "-x", "/etc/opendkim.conf" ]