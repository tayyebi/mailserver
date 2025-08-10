#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PASSFILE="${DIR}/data/dovecot/passwd"

if [ $# -ne 2 ]; then
  echo "Usage: $0 user@example.com 'StrongPassword'"
  exit 1
fi

USER="$1"
PASS="$2"

mkdir -p "$(dirname "$PASSFILE")"
touch "$PASSFILE"

HASH=$(docker exec mail doveadm pw -s SHA512-CRYPT -p "$PASS")
if grep -q "^${USER}:" "$PASSFILE"; then
  sed -i "s|^${USER}:.*|${USER}:${HASH}|" "$PASSFILE"
  echo "Updated password for ${USER}"
else
  echo "${USER}:${HASH}" >> "$PASSFILE"
  echo "Added ${USER}"
fi
