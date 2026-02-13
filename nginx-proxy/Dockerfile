FROM nginx:alpine

# Install openssl for certificate generation and apache2-utils for htpasswd
RUN apk add --no-cache openssl apache2-utils

# Copy nginx configuration
COPY nginx.conf /etc/nginx/nginx.conf

# Use envsubst templates so ports / host can come from .env
COPY default.conf /etc/nginx/templates/default.conf.template

# Create directory for SSL certificates
RUN mkdir -p /etc/nginx/ssl

# Generate self-signed certificate
RUN openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
    -keyout /etc/nginx/ssl/nginx-selfsigned.key \
    -out /etc/nginx/ssl/nginx-selfsigned.crt \
    -subj "/C=US/ST=State/L=City/O=Organization/CN=mailserver"

# Copy entrypoint script for basic auth setup
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 80 443

CMD ["/entrypoint.sh"]
