FROM perl:5.36-slim
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    wget ca-certificates build-essential \
    libmail-milter-perl \
    libmime-tools-perl \
    libdata-uuid-perl \
    libjson-maybexs-perl \
    libfile-slurp-perl \
    libfile-path-perl \
  && rm -rf /var/lib/apt/lists/*

# cpanminus kept for any remaining CPAN-only modules (optional)
RUN wget -qO /usr/local/bin/cpanm https://cpanmin.us && chmod +x /usr/local/bin/cpanm

WORKDIR /opt/pixelmilter

# Copy script to /usr/local/bin because the entrypoint expects that path
COPY pixelmilter.pl /usr/local/bin/pixelmilter.pl
COPY pixelmilter-entrypoint.sh /usr/local/bin/pixelmilter-entrypoint.sh

RUN chmod +x /usr/local/bin/pixelmilter-entrypoint.sh /usr/local/bin/pixelmilter.pl

VOLUME ["/data/pixel","/var/run/pixelmilter"]

CMD ["/usr/local/bin/pixelmilter-entrypoint.sh"]