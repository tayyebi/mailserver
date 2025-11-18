#!/usr/bin/env bash
set -euo pipefail

log() { echo "[postfix] $*"; }
render_template() {
  envsubst < "$1" > "$2"
}

log "Rendering configuration files"
render_template /templates/main.cf.tmpl /etc/postfix/main.cf
# Sanity-check: no merged directives (e.g. missing newline between two keys)
if grep -q '=[^[:space:]].*=[^[:space:]]' /etc/postfix/main.cf; then
	log "Malformed directive detected in main.cf"
	exit 1
fi
render_template /templates/master.cf.tmpl /etc/postfix/master.cf
render_template /templates/virtual_aliases.tmpl /etc/postfix/virtual_aliases
render_template /templates/virtual_domains.tmpl /etc/postfix/virtual_domains
render_template /templates/vmailbox.tmpl /etc/postfix/vmailbox

# Render pixel-content-filter.sh with environment variable substitution
if [ -f /templates/pixel-content-filter.sh.tmpl ]; then
	render_template /templates/pixel-content-filter.sh.tmpl /usr/local/bin/pixel-content-filter.sh
	chmod +x /usr/local/bin/pixel-content-filter.sh
	log "Rendered pixel-content-filter.sh with environment variables"
fi


# Load environment defaults
MAIL_DOMAIN="${MAIL_DOMAIN:-example.com}"
MAIL_HOST="${MAIL_HOST:-mail.${MAIL_DOMAIN}}"
TZ="${TZ:-UTC}"
DOVECOT_AUTH_HOST="${DOVECOT_AUTH_HOST:-dovecot}"
DOVECOT_AUTH_PORT="${DOVECOT_AUTH_PORT:-12345}"
DOVECOT_LMTP_HOST="${DOVECOT_LMTP_HOST:-dovecot}"
DOVECOT_LMTP_PORT="${DOVECOT_LMTP_PORT:-24}"

echo "$TZ" > /etc/timezone || true
ln -fs /usr/share/zoneinfo/${TZ} /etc/localtime

# Compile lookup tables
for f in virtual_aliases virtual_domains vmailbox; do
  postmap "/etc/postfix/$f"
done

# auto upgrade postfix configuration to match the current version
postfix upgrade-configuration

# Copy pixelmilter binary from pixelmilter container if available
# Use docker compose exec from host if docker compose is available, or try docker cp
PIXELMILTER_CONTAINER="pixelmilter"
if [ -f /usr/local/bin/docker-compose ] || command -v docker-compose >/dev/null 2>&1; then
  log "Attempting to copy pixelmilter binary using docker compose"
  if docker-compose exec -T "$PIXELMILTER_CONTAINER" cat /usr/local/bin/pixelmilter > /usr/local/bin/pixelmilter 2>/dev/null; then
    chmod +x /usr/local/bin/pixelmilter
    log "Successfully copied pixelmilter binary"
  else
    log "Warning: Could not copy pixelmilter binary via docker compose"
  fi
elif command -v docker >/dev/null 2>&1; then
  PIXELMILTER_CONTAINER=$(docker ps --filter "name=pixelmilter" --format "{{.Names}}" | head -1)
  if [ -n "$PIXELMILTER_CONTAINER" ]; then
    log "Copying pixelmilter binary from container $PIXELMILTER_CONTAINER"
    if docker cp "${PIXELMILTER_CONTAINER}:/usr/local/bin/pixelmilter" /usr/local/bin/pixelmilter 2>/dev/null; then
      chmod +x /usr/local/bin/pixelmilter
      log "Successfully copied pixelmilter binary"
    else
      log "Warning: Could not copy pixelmilter binary, content filter may not work"
    fi
  fi
else
  # Try to copy from shared volume
  if [ -f /data/pixel/pixelmilter ]; then
    cp /data/pixel/pixelmilter /usr/local/bin/pixelmilter
    chmod +x /usr/local/bin/pixelmilter
    log "Copied pixelmilter binary from shared volume"
  else
    log "Warning: pixelmilter binary not found, content filter may not work"
  fi
fi

# Lint the entire Postfix configuration
log "Running postfix check"
if ! postfix check; then
  log "Postfix config validation failed"
  exit 1
fi

# Launch Postfix in foreground
log "Starting Postfix"
exec /usr/sbin/postfix -vvv start-fg