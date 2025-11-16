FROM perl:5.36-slim
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential libssl-dev \
  && rm -rf /var/lib/apt/lists/*
# cpanminus
RUN apt-get update && apt-get install -y wget && rm -rf /var/lib/apt/lists/*
RUN wget -qO /usr/local/bin/cpanm https://cpanmin.us && chmod +x /usr/local/bin/cpanm
# install runtime deps used by pixelmilter.pl
RUN cpanm --notest Mail::Milter::Simple MIME::Parser MIME::Entity JSON::MaybeXS Data::UUID File::Slurp File::Path
WORKDIR /opt/pixelmilter
COPY pixelmilter.pl /opt/pixelmilter/pixelmilter.pl
COPY pixelmilter-entrypoint.sh /opt/pixelmilter/pixelmilter-entrypoint.sh
RUN chmod +x /opt/pixelmilter/pixelmilter.pl /opt/pixelmilter/pixelmilter-entrypoint.sh
VOLUME ["/data/pixel","/var/run/pixelmilter"]
EXPOSE 0
CMD ["/opt/pixelmilter/pixelmilter-entrypoint.sh"]