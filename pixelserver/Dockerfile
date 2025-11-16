FROM perl:5.36-slim
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    wget ca-certificates \
    libmojolicious-perl \
    libjson-maybexs-perl \
    libfile-slurp-perl \
    libfile-path-perl \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/pixelserver
COPY app.pl /opt/pixelserver/app.pl
RUN chmod +x /opt/pixelserver/app.pl

VOLUME ["/data/pixel"]
EXPOSE 8443
CMD ["perl", "/opt/pixelserver/app.pl"]