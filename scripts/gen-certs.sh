#!/usr/bin/env bash
# ~/d/mailserver/scripts/gen-certs.sh
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${DIR}/.env"
OUT_DIR="${DIR}/data/ssl"
DAYS=825
BITS=4096
FORCE=0
ADD_SANS=""
GENERATE_DH=1

bold(){ printf '\033[1m%s\033[0m\n' "$*"; }
log(){ printf '[cert] %s\n' "$*"; }
fail(){ printf '[FAIL] %s\n' "$*" >&2; exit 1; }

usage(){
  cat <<EOF
Usage: $(basename "$0") [--force] [--days N] [--bits N] [--sans 'DNS:alt1,DNS:alt2,IP:1.2.3.4'] [--no-dh]
  --force          overwrite existing cert/key
  --days N         validity in days (default ${DAYS})
  --bits N         RSA key size (default ${BITS})
  --sans STRING    extra SubjectAltNames (comma-separated)
  --no-dh          skip generating dhparam.pem
Reads MAIL_HOST and MAIL_DOMAIN from ./.env
Outputs to ${OUT_DIR}/mailserver.{crt,key}
EOF
}

# Load .env if present
if [[ -f "$ENV_FILE" ]]; then
  # shellcheck disable=SC2046
  export $(grep -E '^[A-Z0-9_]+=' "$ENV_FILE" | sed 's/#.*//' | xargs -I{} echo {})
fi

# Parse args
while [[ $# -gt 0 ]]; do
  case "$1" in
    --force) FORCE=1; shift ;;
    --days) DAYS="$2"; shift 2 ;;
    --bits) BITS="$2"; shift 2 ;;
    --sans) ADD_SANS="$2"; shift 2 ;;
    --no-dh) GENERATE_DH=0; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage; exit 1 ;;
  esac
done

[[ -n "${MAIL_HOST:-}" ]] || fail "MAIL_HOST is not set (.env)"
[[ -n "${MAIL_DOMAIN:-}" ]] || fail "MAIL_DOMAIN is not set (.env)"

mkdir -p "$OUT_DIR"
CRT="${OUT_DIR}/mailserver.crt"
KEY="${OUT_DIR}/mailserver.key"
CSR="${OUT_DIR}/mailserver.csr"
EXT="${OUT_DIR}/san.ext"
DHP="${OUT_DIR}/dhparam.pem"

if [[ -s "$CRT" || -s "$KEY" ]] && [[ $FORCE -eq 0 ]]; then
  log "Existing cert/key found. Use --force to overwrite."
  echo "  $CRT"
  echo "  $KEY"
  exit 0
fi

SAN_LIST="DNS:${MAIL_HOST},DNS:${MAIL_DOMAIN}"
if [[ -n "$ADD_SANS" ]]; then
  SAN_LIST="${SAN_LIST},${ADD_SANS}"
fi

cat > "$EXT" <<EOF
basicConstraints=CA:FALSE
subjectAltName=${SAN_LIST}
keyUsage = digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
EOF

bold "Generating RSA ${BITS}-bit key"
openssl genrsa -out "$KEY" "$BITS" >/dev/null 2>&1
chmod 640 "$KEY"

bold "Creating CSR for ${MAIL_HOST}"
openssl req -new -key "$KEY" -subj "/CN=${MAIL_HOST}" -out "$CSR" >/dev/null 2>&1

bold "Self-signing certificate (${DAYS} days) with SANs: ${SAN_LIST}"
openssl x509 -req -in "$CSR" -signkey "$KEY" -days "$DAYS" -sha256 -extfile "$EXT" -out "$CRT" >/dev/null 2>&1

log "Certificate generated:"
openssl x509 -noout -subject -issuer -dates -fingerprint -sha256 -in "$CRT"

if [[ $GENERATE_DH -eq 1 ]]; then
  bold "Generating DH parameters (2048 bits) — one-time, may take a minute"
  openssl dhparam -out "$DHP" 2048 >/dev/null 2>&1
  log "DH params at $DHP"
fi

log "Done. Mounting path matches docker-compose (data/ssl). Restart mail container to load:"
echo "  docker compose restart mail"
