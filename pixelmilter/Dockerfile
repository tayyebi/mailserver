FROM perl:5.36-slim
ENV DEBIAN_FRONTEND=noninteractive

# Install runtime deps from apt (prefer Debian packages over CPAN)
RUN apt-get update && apt-get install -y --no-install-recommends \
    wget build-essential ca-certificates \
    libmail-milter-perl \
    libmime-tools-perl \
    libdata-uuid-perl \
    libjson-maybexs-perl \
    libfile-slurp-perl \
    libfile-path-perl \
  && rm -rf /var/lib/apt/lists/*

# Install cpanminus for any CPAN-only modules (optional)
RUN wget -qO /usr/local/bin/cpanm https://cpanmin.us && chmod +x /usr/local/bin/cpanm

WORKDIR /opt/pixelmilter

# Copy runtime scripts into image
COPY pixelmilter.pl /opt/pixelmilter/pixelmilter.pl
COPY pixelmilter-entrypoint.sh /usr/local/bin/pixelmilter-entrypoint.sh
RUN chmod +x /usr/local/bin/pixelmilter-entrypoint.sh /opt/pixelmilter/pixelmilter.pl

VOLUME ["/data/pixel","/var/run/pixelmilter"]

# Entrypoint will create socket dir and exec the Perl milter
CMD ["/usr/local/bin/pixelmilter-entrypoint.sh"]
