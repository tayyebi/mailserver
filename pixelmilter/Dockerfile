FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
      perl \
      cpanminus \
      build-essential \
      libmail-milter-perl \
      libmime-tools-perl \
      libdata-uuid-perl \
      libjson-maybexs-perl \
      libfile-slurp-perl \
      libfile-path-perl \
      libio-socket-ssl-perl \
      libio-string-perl \
      libencode-perl \
      ca-certificates \
      gosu \
      tzdata \
    && cpanm --notest Mail::Milter::Server \
    && apt-get remove -y cpanminus build-essential \
    && apt-get autoremove -y \
    && rm -rf /var/lib/apt/lists/* /root/.cpanm

# Create pixel user
RUN useradd -r -s /usr/sbin/nologin pixel \
    && mkdir -p /opt/pixelmilter /data/pixel /var/run/pixelmilter \
    && chown -R pixel:pixel /opt/pixelmilter /data/pixel /var/run/pixelmilter

WORKDIR /opt/pixelmilter

COPY pixelmilter.pl /usr/local/bin/pixelmilter.pl
COPY pixelmilter-entrypoint.sh /usr/local/bin/pixelmilter-entrypoint.sh

RUN chmod +x /usr/local/bin/pixelmilter.pl /usr/local/bin/pixelmilter-entrypoint.sh

ENTRYPOINT ["/usr/local/bin/pixelmilter-entrypoint.sh"]
CMD ["/usr/local/bin/pixelmilter.pl"]
