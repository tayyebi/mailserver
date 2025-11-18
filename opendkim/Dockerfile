FROM debian:bookworm
LABEL maintainer="Mohammad R. Tayyebi <m@tyyi.net>" description="OpenDKIM signing service" version="1.1"

ARG DEBIAN_FRONTEND=noninteractive
ARG TZ=UTC
ENV TZ=${TZ}

RUN apt-get update \
 && apt-get install -y opendkim tzdata openssl \
 && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime \
 && dpkg-reconfigure -f noninteractive tzdata \
 && rm -rf /var/lib/apt/lists/*

COPY opendkim.conf /etc/opendkim/opendkim.conf