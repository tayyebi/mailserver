FROM debian:bookworm-slim
LABEL maintainer="MohammadReza" description="OpenDKIM signing service" version="1.1"

ARG DEBIAN_FRONTEND=noninteractive
ARG TZ=UTC
ENV TZ=${TZ}

RUN apt-get update \
 && apt-get install -y --no-install-recommends opendkim tzdata openssl \
 && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime \
 && dpkg-reconfigure -f noninteractive tzdata \
 && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /etc/opendkim/keys \
 && chown -R opendkim:opendkim /etc/opendkim

# Config and keys live directly under /etc/opendkim
VOLUME ["/etc/opendkim", "/etc/opendkim/keys"]

EXPOSE 8891

ENTRYPOINT ["opendkim", "-f", "-x", "/etc/opendkim/opendkim.conf"]