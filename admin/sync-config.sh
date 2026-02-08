#!/bin/bash
# Script to sync admin panel SQLite database to Postfix/Dovecot configuration files

ADMIN_DB="/var/www/html/database/database.sqlite"
POSTFIX_VIRTUAL_DOMAINS="/etc/postfix/virtual_domains"
POSTFIX_VIRTUAL_MAILBOX="/etc/postfix/vmailbox"
POSTFIX_VIRTUAL_ALIASES="/etc/postfix/virtual_aliases"
DOVECOT_PASSWD="/etc/dovecot/passwd"

# Wait for database to be available
while [ ! -f "$ADMIN_DB" ]; do
    echo "Waiting for admin database..."
    sleep 2
done

# Export virtual domains
sqlite3 "$ADMIN_DB" "SELECT domain FROM domains WHERE active=1;" > "$POSTFIX_VIRTUAL_DOMAINS"

# Export virtual mailboxes (email -> maildir path)
sqlite3 "$ADMIN_DB" "SELECT email || ' ' || username || '/' FROM email_accounts WHERE active=1;" > "$POSTFIX_VIRTUAL_MAILBOX"

# Export virtual aliases
sqlite3 "$ADMIN_DB" "SELECT source || ' ' || destination FROM aliases WHERE active=1;" > "$POSTFIX_VIRTUAL_ALIASES"

# Export dovecot passwords (email:password_hash)
sqlite3 "$ADMIN_DB" "SELECT email || ':' || password FROM email_accounts WHERE active=1;" > "$DOVECOT_PASSWD"

# Set permissions
chmod 644 "$POSTFIX_VIRTUAL_DOMAINS" "$POSTFIX_VIRTUAL_MAILBOX" "$POSTFIX_VIRTUAL_ALIASES"
chmod 600 "$DOVECOT_PASSWD"

# Reload services
postfix reload 2>/dev/null || true
doveadm reload 2>/dev/null || true

echo "Configuration synced from admin database"
