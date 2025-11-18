#!/bin/bash
# Postfix content filter wrapper for pixelmilter
# This script is called by Postfix to filter emails

set -euo pipefail

# Read email from stdin and pass to pixelmilter in content filter mode
# stdin/stdout are automatically handled by exec
exec /usr/local/bin/pixelmilter \
    --content-filter-mode true \
    --pixel-base-url "${PIXEL_BASE_URL:-https://localhost:8443/pixel?id=}" \
    --tracking-requires-opt-in "${TRACKING_REQUIRES_OPT_IN:-false}" \
    --opt-in-header "${OPT_IN_HEADER:-X-Track-Open}" \
    --disclosure-header "${DISCLOSURE_HEADER:-X-Tracking-Notice}" \
    --inject-disclosure "${INJECT_DISCLOSURE:-true}" \
    --data-dir "${DATA_DIR:-/data/pixel}" \
    --footer-html-file "${FOOTER_HTML_FILE:-/opt/pixelmilter/domain-wide-footer.html}" \
    --log-level "${LOG_LEVEL:-info}"

