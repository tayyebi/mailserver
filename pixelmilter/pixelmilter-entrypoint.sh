#!/bin/bash
set -euo pipefail

# create expected dirs (if they are bind-mounted these calls are harmless)
mkdir -p /data/pixel

# chown/chmod only if we are root. If the host mount prevents changing ownership,
# suppress errors to avoid noisy logs or fatal exit.
if [ "$(id -u)" = "0" ]; then
  chown -R pixel:pixel /data/pixel 2>/dev/null || true
fi

ADDRESS=${PIXEL_MILTER_ADDRESS:-0.0.0.0:8892}

# Run the binary directly - user switching can be handled by Docker USER directive if needed
exec /usr/local/bin/pixelmilter --address "$ADDRESS" "$@" 2>&1