#!/bin/bash
set -euo pipefail

# Create expected dirs (if they are bind-mounted these calls are harmless)
mkdir -p /data/pixel

# chown/chmod only if we are root. If the host mount prevents changing ownership,
# suppress errors to avoid noisy logs or fatal exit.
if [ "$(id -u)" = "0" ]; then
  chown -R pixelserver:pixelserver /data/pixel 2>/dev/null || true
  chmod 0755 /data/pixel 2>/dev/null || true
fi

# Run the binary directly - user switching can be handled by Docker USER directive if needed
exec /usr/local/bin/pixelserver "$@" 2>&1