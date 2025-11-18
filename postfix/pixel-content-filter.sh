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

# Process email through pixelmilter and reinject via SMTP to dedicated reinjection port
# pixelmilter reads from stdin, modifies email, writes to stdout
# We pipe the output to a Python script that sends it via SMTP to port 10025 (no content_filter)
/usr/local/bin/pixelmilter "${ARGS[@]}" | python3 -c "
import sys
import socket

# Read email from stdin
email = sys.stdin.read()

# Extract recipient from To header
to_addr = None
for line in email.split('\n'):
    if line.lower().startswith('to:'):
        to_addr = line.split(':', 1)[1].strip()
        # Remove angle brackets if present
        to_addr = to_addr.strip('<>')
        break

if not to_addr:
    sys.stderr.write('No To header found\n')
    sys.exit(1)

# Connect to reinjection service
sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.connect(('127.0.0.1', 10025))

# Send SMTP commands
sock.sendall(b'EHLO localhost\r\n')
response = sock.recv(1024)

sock.sendall(f'MAIL FROM:<noreply@localhost>\r\n'.encode())
response = sock.recv(1024)

sock.sendall(f'RCPT TO:<{to_addr}>\r\n'.encode())
response = sock.recv(1024)

sock.sendall(b'DATA\r\n')
response = sock.recv(1024)

# Send email content
sock.sendall(email.encode())
sock.sendall(b'\r\n.\r\n')
response = sock.recv(1024)

sock.sendall(b'QUIT\r\n')
sock.close()
"
