# Port Configuration Guide

This document describes the standardized port configuration for the mail server stack.

## Port Standardization

All ports are now defined as environment variables in `.env.example` for easy configuration and consistency across the stack.

## Port Categories

### 1. External Ports (Host → Container)

These ports are exposed to the host machine and accessible from outside the Docker network.

#### HTTP/HTTPS (Reverse Proxy)
- `HTTP_PORT=80` - HTTP traffic (redirects to HTTPS)
- `HTTPS_PORT=443` - HTTPS traffic (admin panel, pixel server, reports)

#### SMTP (Postfix)
- `SMTP_PORT=25` - Standard SMTP
- `SUBMISSION_PORT=587` - Mail submission (STARTTLS)
- `SMTPS_PORT=465` - SMTP over SSL

#### IMAP (Dovecot)
- `IMAP_PORT=143` - Standard IMAP
- `IMAPS_PORT=993` - IMAP over SSL

#### POP3 (Dovecot)
- `POP3_PORT=110` - Standard POP3
- `POP3S_PORT=995` - POP3 over SSL

### 2. Internal Ports (Docker Network Only)

These ports are only accessible within the Docker `mailnet` network and not exposed to the host.

#### OpenDKIM
- `OPENDKIM_PORT=8891` - DKIM signing service

#### PixelMilter
- `PIXEL_MILTER_PORT=8892` - Email tracking milter

#### PixelServer
- `PIXEL_SERVER_HTTPS_PORT=8443` - Tracking pixel endpoint (HTTPS)
- `PIXEL_SERVER_REPORTS_PORT=8444` - Reports API (HTTP)

#### Dovecot Internal
- `DOVECOT_LMTP_PORT=24` - Local Mail Transfer Protocol
- `DOVECOT_AUTH_PORT=12345` - SASL authentication

#### Admin Panel Internal
- `ADMIN_INTERNAL_PORT=80` - Laravel application (proxied via nginx-proxy)

## Network Addresses

All services use static IP addresses within the `172.18.0.0/16` subnet:

- `DOVECOT_IP=172.18.0.5`
- `OPENDKIM_IP=172.18.0.6`
- `POSTFIX_IP=172.18.0.7`
- `PIXEL_MILTER_IP=172.18.0.20`
- `PIXEL_SERVER_IP=172.18.0.21`
- `ADMIN_IP=172.18.0.31`
- `PROXY_IP=172.18.0.40`

## Port Usage Matrix

| Service | Internal Port | External Port | Protocol | Purpose |
|---------|--------------|---------------|----------|---------|
| Nginx Proxy | - | 80, 443 | HTTP/HTTPS | Reverse proxy |
| Postfix | - | 25, 587, 465 | SMTP | Mail sending/receiving |
| Dovecot | 24, 12345 | 143, 993, 110, 995 | IMAP/POP3 | Mail access |
| OpenDKIM | 8891 | - | TCP | Email signing |
| PixelMilter | 8892 | - | TCP | Tracking milter |
| PixelServer | 8443, 8444 | - (via proxy) | HTTPS/HTTP | Tracking & reports |
| Admin Panel | 80 | - (via proxy) | HTTP | Administration |

## Service Communication

### Postfix → Other Services
- Postfix → OpenDKIM: `${OPENDKIM_IP}:${OPENDKIM_PORT}`
- Postfix → PixelMilter: `${PIXEL_MILTER_IP}:${PIXEL_MILTER_PORT}`
- Postfix → Dovecot LMTP: `${DOVECOT_IP}:${DOVECOT_LMTP_PORT}`
- Postfix → Dovecot Auth: `${DOVECOT_IP}:${DOVECOT_AUTH_PORT}`

### Nginx Proxy → Backend Services
- Proxy → Admin Panel: `http://mailserver_admin:${ADMIN_INTERNAL_PORT}`
- Proxy → PixelServer (tracking): `https://pixelserver:${PIXEL_SERVER_HTTPS_PORT}`
- Proxy → PixelServer (reports): `http://pixelserver:${PIXEL_SERVER_REPORTS_PORT}`

## Customizing Ports

To change any port, update the corresponding environment variable in your `.env` file:

```bash
# Example: Change HTTPS port to 8443
HTTPS_PORT=8443

# Example: Change SMTP submission port to 2587
SUBMISSION_PORT=2587
```

All port variables have sensible defaults, so you only need to override them if needed.

## Security Considerations

### Exposed Ports (External Access)
Only essential mail and web ports are exposed:
- Ports 80, 443 (web)
- Ports 25, 587, 465 (SMTP)
- Ports 143, 993, 110, 995 (IMAP/POP3)

### Internal Ports (Network Isolation)
All internal service ports are isolated within the Docker network:
- No direct external access to admin panel
- No direct external access to pixel server
- No direct external access to milter services
- No direct external access to DKIM service

This ensures that only the reverse proxy and mail services are accessible from outside, while internal services remain protected.

## Troubleshooting

### Port Already in Use

If you get a "port already in use" error:

1. Check what's using the port:
   ```bash
   sudo lsof -i :PORT_NUMBER
   ```

2. Either stop the conflicting service or change the port in `.env`:
   ```bash
   # Change conflicting port
   HTTP_PORT=8080
   ```

### Service Cannot Connect

If services can't communicate:

1. Verify port variables are set correctly in `.env`
2. Check service logs:
   ```bash
   docker-compose logs SERVICE_NAME
   ```
3. Verify network connectivity:
   ```bash
   docker-compose exec SERVICE_NAME ping OTHER_SERVICE
   ```

## Port Reference Quick Guide

```
External Access:
  80, 443      → Reverse Proxy (Admin, Reports, Pixel)
  25, 587, 465 → SMTP (Postfix)
  143, 993     → IMAP (Dovecot)
  110, 995     → POP3 (Dovecot)

Internal Only:
  8891         → OpenDKIM
  8892         → PixelMilter
  8443, 8444   → PixelServer
  24, 12345    → Dovecot Internal
  80           → Admin Panel Internal
```
