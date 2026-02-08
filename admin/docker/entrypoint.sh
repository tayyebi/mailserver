#!/bin/bash
set -e

echo "[admin] Starting initialization..."

# Add www-data to docker group if it exists (for Docker socket access)
if getent group docker > /dev/null 2>&1; then
    usermod -aG docker www-data 2>/dev/null || true
fi

# Create required directories first
mkdir -p /var/www/html/database
mkdir -p /var/www/html/storage/app/mail-config/opendkim/keys
mkdir -p /var/www/html/storage/app/mail-config/postfix
mkdir -p /var/www/html/storage/app/mail-config/dovecot
mkdir -p /var/www/html/storage/framework/cache
mkdir -p /var/www/html/storage/framework/sessions
mkdir -p /var/www/html/storage/framework/views
mkdir -p /var/www/html/bootstrap/cache

# Set proper permissions
chown -R www-data:www-data /var/www/html/database 2>/dev/null || true
chown -R www-data:www-data /var/www/html/storage 2>/dev/null || true
chown -R www-data:www-data /var/www/html/bootstrap/cache 2>/dev/null || true

# Create .env if it doesn't exist
ENV_CREATED=false
if [ ! -f /var/www/html/.env ]; then
    echo "[admin] Creating .env file..."
    ENV_CREATED=true
    
    # Generate a random APP_KEY
    APP_KEY_GENERATED=$(openssl rand -base64 32)
    
    cat > /var/www/html/.env << EOF
APP_NAME="${APP_NAME:-Mail Server Admin}"
APP_ENV=${APP_ENV:-production}
APP_KEY=base64:${APP_KEY_GENERATED}
APP_DEBUG=${APP_DEBUG:-false}
APP_URL=${APP_URL:-http://localhost}

LOG_CHANNEL=stack
LOG_LEVEL=debug

DB_CONNECTION=${DB_CONNECTION:-sqlite}
DB_DATABASE=/var/www/html/database/database.sqlite

BROADCAST_DRIVER=log
CACHE_DRIVER=file
FILESYSTEM_DISK=local
QUEUE_CONNECTION=${QUEUE_CONNECTION:-database}
SESSION_DRIVER=file
SESSION_LIFETIME=120

MAIL_MAILER=log
EOF
    
    chmod 644 /var/www/html/.env
    echo "[admin] ✓ .env file created with auto-generated APP_KEY"
fi

# Run migrations
echo "[admin] Running database migrations..."
php artisan migrate --force --no-interaction || true

# Cache configuration  
echo "[admin] Caching configuration..."
php artisan config:clear || true
php artisan cache:clear || true
php artisan config:cache || true
php artisan route:cache || true

echo "[admin] Initialization complete. Starting services..."

# Start supervisor (which runs nginx and php-fpm)
exec /usr/bin/supervisord -c /etc/supervisor/conf.d/supervisord.conf
