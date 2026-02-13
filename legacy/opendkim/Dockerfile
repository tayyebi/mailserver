FROM alpine:3.21

LABEL maintainer="Mohammad R. Tayyebi <m@tyyi.net>"
LABEL description="OpenDKIM signing service"
LABEL version="1.1"

ARG TZ=UTC
ENV TZ=${TZ}

RUN apk add --no-cache \
        opendkim \
        opendkim-utils \
        tzdata \
        openssl \
    && ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime
