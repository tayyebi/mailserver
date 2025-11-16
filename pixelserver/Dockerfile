FROM perl:5.36-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    wget \
  && rm -rf /var/lib/apt/lists/*

# cpanminus
RUN wget -qO /usr/local/bin/cpanm https://cpanmin.us && chmod +x /usr/local/bin/cpanm

# install Mojolicious for lightweight web app
RUN cpanm --notest Mojolicious JSON::MaybeXS

WORKDIR /opt/pixelserver

COPY app.pl /opt/pixelserver/app.pl
RUN chmod +x /opt/pixelserver/app.pl

VOLUME ["/data/pixel"]

EXPOSE 8443

CMD ["perl", "/opt/pixelserver/app.pl"]