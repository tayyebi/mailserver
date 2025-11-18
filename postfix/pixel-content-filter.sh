#!/bin/bash
# Postfix content filter wrapper for pixelmilter
# This script is called by Postfix to filter emails and reinject them

set -euo pipefail

# Build command arguments
ARGS=(
    --content-filter-mode
    --pixel-base-url "${PIXEL_BASE_URL:-https://localhost:8443/pixel?id=}"
    --opt-in-header "${OPT_IN_HEADER:-X-Track-Open}"
    --disclosure-header "${DISCLOSURE_HEADER:-X-Tracking-Notice}"
    --data-dir "${DATA_DIR:-/data/pixel}"
    --footer-html-file "${FOOTER_HTML_FILE:-/opt/pixelmilter/domain-wide-footer.html}"
    --log-level "${LOG_LEVEL:-warn}"
)

# Add boolean flags only if they are true
if [ "${TRACKING_REQUIRES_OPT_IN:-false}" = "true" ] || [ "${TRACKING_REQUIRES_OPT_IN:-false}" = "1" ]; then
    ARGS+=(--tracking-requires-opt-in)
fi

if [ "${INJECT_DISCLOSURE:-true}" = "true" ] || [ "${INJECT_DISCLOSURE:-true}" = "1" ]; then
    ARGS+=(--inject-disclosure)
fi

# Process email through pixelmilter and reinject via sendmail
# pixelmilter reads from stdin, modifies email, writes to stdout
# We pipe the output to sendmail to reinject into Postfix
/usr/local/bin/pixelmilter "${ARGS[@]}" | /usr/sbin/sendmail -G -i "$@"
