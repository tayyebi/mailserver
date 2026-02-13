FROM nginx:alpine

RUN apk add --no-cache openssl apache2-utils
RUN mkdir -p /etc/nginx/ssl

COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
COPY nginx.conf /etc/nginx/nginx.conf
COPY default.conf /etc/nginx/templates/default.conf.template

EXPOSE 80 443
