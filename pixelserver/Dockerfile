FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
      perl \
      libmojolicious-perl \
      libjson-maybexs-perl \
      libfile-slurp-perl \
      libfile-path-perl \
      libencode-perl \
      ca-certificates \
      openssl \
      curl \
      tzdata \
    && rm -rf /var/lib/apt/lists/*

# Create pixelserver user
RUN useradd -r -s /usr/sbin/nologin pixelserver \
    && mkdir -p /opt/pixelserver /data/pixel \
    && chown -R pixelserver:pixelserver /opt/pixelserver /data/pixel

WORKDIR /opt/pixelserver

COPY app.pl /opt/pixelserver/app.pl
RUN chmod +x /opt/pixelserver/app.pl

ENV PIXEL_TLS_CERT="/etc/ssl/certs/pixelserver.crt"
ENV PIXEL_TLS_KEY="/etc/ssl/private/pixelserver.key"

EXPOSE 8443

USER pixelserver

CMD ["perl", "/opt/pixelserver/app.pl"]
