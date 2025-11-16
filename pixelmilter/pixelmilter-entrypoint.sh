#!/bin/bash
set -euo pipefail

mkdir -p /data/pixel
mkdir -p /var/run/pixelmilter
chown -R nobody:nogroup /data/pixel || true

SOCKET=${PIXEL_MILTER_SOCKET:-/var/run/pixelmilter/pixel.sock}

# ensure socket dir exists and permissions suitable for Docker-mounted volume
mkdir -p "$(dirname "$SOCKET")"
chmod 0777 "$(dirname "$SOCKET")" || true

exec /usr/local/bin/pixelmilter.pl --socket "$SOCKET"