#!/bin/bash
set -e

echo "[admin] Starting initialization..."

# Create required directories
mkdir -p /var/www/html/database
mkdir -p /var/www/html/storage/app/mail-config/opendkim/keys
mkdir -p /var/www/html/storage/app/mail-config/postfix
mkdir -p /var/www/html/storage/app/mail-config/dovecot

# Set proper permissions
chown -R www-data:www-data /var/www/html/database
chown -R www-data:www-data /var/www/html/storage

# Generate APP_KEY if not set or is the default placeholder
if [ -z "$APP_KEY" ] || [ "$APP_KEY" = "base64:CHANGEME_GENERATE_NEW_KEY_WITH_ARTISAN" ]; then
    echo "[admin] Generating application key..."
    php artisan key:generate --force
fi

# Run migrations
echo "[admin] Running database migrations..."
php artisan migrate --force --no-interaction

# Cache configuration
echo "[admin] Caching configuration..."
php artisan config:cache
php artisan route:cache

echo "[admin] Initialization complete. Starting services..."

# Start supervisor (which runs nginx and php-fpm)
exec /usr/bin/supervisord -c /etc/supervisor/conf.d/supervisord.conf
