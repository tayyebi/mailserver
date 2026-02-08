# 📧 Dockerized Postfix + Dovecot + OpenDKIM Mailserver

Self-contained, persistent mail stack with web administration:
- Postfix SMTP (25, 587) with DKIM signing
- Dovecot IMAP/IMAPS and SASL auth
- Dovecot LMTP for final delivery to Maildir
- **Web Admin Panel** for managing domains, email accounts, aliases, and DKIM keys
- Shared TLS certificates (read-only) for Postfix and Dovecot
- Multi‑domain sending/signing with simple web interface

---

## System Architecture

```mermaid
graph TB
    subgraph "Mail Server Stack"
        Proxy[Nginx Reverse Proxy<br/>Ports 80/443]
        Admin[Web Admin Panel<br/>Laravel + SQLite]
        DB[(SQLite<br/>Database)]
        Postfix[Postfix<br/>SMTP Server]
        Dovecot[Dovecot<br/>IMAP Server]
        OpenDKIM[OpenDKIM<br/>Email Signing]
        PixelMilter[Pixel Milter<br/>Tracking]
        PixelServer[Pixel Server<br/>Analytics]
    end
    
    User[Administrator] -->|HTTPS 443| Proxy
    Proxy -->|/admin| Admin
    Proxy -->|/pixel| PixelServer
    Proxy -->|/admin/reports| PixelServer
    
    Admin -->|Manages| DB
    DB -->|Virtual Config| Postfix
    DB -->|Auth & Mailbox| Dovecot
    
    EmailClient[Email Client] -->|SMTP 25,587,465| Postfix
    EmailClient -->|IMAP 143,993| Dovecot
    
    Postfix -->|Port 8891| OpenDKIM
    Postfix -->|Port 8892| PixelMilter
    Postfix -->|LMTP Port 24| Dovecot
    PixelMilter -->|Pixel Data| PixelServer
    
    style Proxy fill:#f093fb
    style Admin fill:#667eea
    style DB fill:#48bb78
    style Postfix fill:#ed8936
    style Dovecot fill:#4299e1
    style PixelServer fill:#f56565
```

---

## Admin Panel Screenshots

### Dashboard
![Dashboard](https://github.com/user-attachments/assets/d75f1bb9-fcfe-41d9-a417-9cf63e1c4e3a)

### Create Email Account
![Create Email Account](https://github.com/user-attachments/assets/af376448-9e2b-4002-a196-8d9de45dadd8)

---

## Features

### Web Administration Panel
- 🎨 **Clean UI** - Simple, responsive interface with NO JavaScript
- 🏢 **Domain Management** - Add and configure email domains
- 🔑 **DKIM Management** - Generate and manage DKIM signing keys
- 👥 **Email Accounts** - Create mailboxes with passwords and quotas
- 🔄 **Aliases** - Set up email forwarding and catch-all addresses
- 📊 **Dashboard** - View statistics at a glance
- 💾 **SQLite Database** - Lightweight, file-based storage

### Mail Server Features
- 📧 SMTP sending and receiving (ports 25, 587, 465)
- 📬 IMAP access (ports 143, 993)
- 🔐 DKIM email signing for authenticity
- 📊 Email tracking with pixel insertion
- 🗂️ Maildir storage format

---

## Prerequisites

- Docker and Docker Compose

---

## Install

```bash
# Clone and enter
git clone https://github.com/tayyebi/mailserver mailserver
cd mailserver

# Create config from template
cp .env.example .env
# Edit .env: Set MAIL_DOMAIN, MAIL_HOST, TZ, and APP_KEY
# Generate APP_KEY with: openssl rand -base64 32

# Start all services
docker compose up -d
```

The system will automatically:
- Generate self‑signed TLS certificates if missing
- Create all required data directories
- Initialize the admin panel database
- Start all mail services

The admin panel and pixel server will be available at:
- **Admin Panel**: `https://your-server/admin`
- **Pixel Tracking**: `https://your-server/pixel`
- **Reports API**: `https://your-server/admin/reports`

All services are accessible through the reverse proxy on standard HTTPS port 443.

---

## DNS checklist

- A: MAIL_HOST → server public IP
- MX: your domain(s) → MAIL_HOST
- PTR: reverse DNS → MAIL_HOST
- SPF (TXT at domain): `v=spf1 a mx ~all`
- DKIM (TXT at default._domainkey.domain): value from `data/opendkim/keys/<domain>/default.txt`
- DMARC (TXT at _dmarc.domain): `v=DMARC1; p=quarantine; rua=mailto:dmarc@domain; fo=1`

---

## Data layout (persistent)

- data/ssl — TLS certs/keys (shared read‑only by Postfix and Dovecot)
- data/postfix — Postfix configs and maps (virtual, virtual_domains)
- data/spool — Postfix queue
- data/opendkim/keys — DKIM keys (per domain)
- data/opendkim/conf — DKIM tables and config
- data/dovecot — Dovecot state/indexes
- data/dovecot-conf — Dovecot config (dovecot.conf, users)
- data/mail — Maildir storage: data/mail/<domain>/<user>/{cur,new,tmp}
- data/pixel — Pixel tracking data and socket directory

---

## Pixel Tracking (pixelmilter)

The mailserver includes pixel tracking functionality that injects tracking pixels and domain-wide footers into HTML emails to track opens.

### Configuration

Pixel tracking can be configured via environment variables in your `.env` file:

- `TRACKING_REQUIRES_OPT_IN` (default: `false`): If `false`, tracking is enabled by default for all HTML emails. If `true`, tracking only occurs when emails include an opt-in header (see `OPT_IN_HEADER`).
- `OPT_IN_HEADER` (default: `X-Track-Open`): Header name to check for opt-in when `TRACKING_REQUIRES_OPT_IN=true`.
- `PIXEL_BASE_URL` (default: `https://${MAIL_HOST}:8443/pixel?id=`): Base URL for tracking pixels.
- `DISCLOSURE_HEADER` (default: `X-Tracking-Notice`): Header name for privacy disclosure.
- `INJECT_DISCLOSURE` (default: `true`): Whether to inject disclosure header into tracked emails.
- `PIXEL_MILTER_ADDRESS` (default: `0.0.0.0:8892`): Address and port for the milter service.

**Example `.env` configuration for domain-wide tracking (default):**
```bash
TRACKING_REQUIRES_OPT_IN=false
PIXEL_BASE_URL=https://mail.gordarg.com:8443/pixel?id=
```

**Example `.env` configuration for opt-in only tracking:**
```bash
TRACKING_REQUIRES_OPT_IN=true
OPT_IN_HEADER=X-Track-Open
```

### Verifying pixelmilter Configuration

To ensure pixelmilter is correctly applied to the whole project:

```bash
# Check pixelmilter container status
docker compose ps pixelmilter

# Check pixelmilter logs
docker compose logs pixelmilter
```

This should show:
- pixelmilter container is running
- No errors in logs
- Postfix configuration includes pixelmilter

### Updating Configuration Files (.cf files)

Postfix configuration files (`main.cf`, `master.cf`) are generated from templates (`.tmpl` files) when the container starts. To ensure configuration files are updated after editing templates:

**Rebuild and restart Postfix:**
```bash
docker compose build postfix
docker compose restart postfix
```

**Or restart all services:**
```bash
docker compose restart
```

**Reload only (if no template changes):**
```bash
docker compose exec postfix postfix reload
```

### Configuration File Locations

- **Templates**: `postfix/main.cf.tmpl`, `postfix/master.cf.tmpl`
- **Rendered configs**: Generated inside the Postfix container at `/etc/postfix/main.cf`, `/etc/postfix/master.cf`
- **Pixelmilter connection**: TCP port 8892 (configurable via `PIXEL_MILTER_ADDRESS` environment variable)

### Pixelmilter Integration

Pixelmilter is configured in `postfix/main.cf.tmpl`:
- `smtpd_milters` includes `inet:${PIXEL_MILTER_IP}:8892` for incoming mail
- Pixelmilter listens on TCP port 8892 (configurable via `PIXEL_MILTER_ADDRESS` environment variable)
- Postfix connects to pixelmilter via the Docker network using the `PIXEL_MILTER_IP` address

After modifying `main.cf.tmpl` or `master.cf.tmpl`, rebuild and restart Postfix:
```bash
docker compose build postfix && docker compose restart postfix
```

---

## Administration Panel

The mailserver includes a simple Laravel-based web admin panel for managing domains, email accounts, and aliases. All services are accessed through a dedicated Nginx reverse proxy.

### Access the Admin Panel

1. **Start all services**:
   ```bash
   docker-compose up -d
   ```

2. **Access the interface**:
   - Admin Panel: `https://your-server/admin`
   - Pixel Tracking: `https://your-server/pixel`
   - Reports API: `https://your-server/admin/reports`

All services use self-signed SSL certificates and are accessible through standard HTTPS port 443.

### Features

- **No Authentication Required** - Direct access for simplicity
- **No JavaScript** - Pure HTML forms, works everywhere
- **SQLite Database** - Lightweight file-based storage
- **Reverse Proxy** - Single entry point on standard ports 80/443
- **Self-Signed SSL** - HTTPS encryption without third-party certificates
- **Full CRUD Operations**:
  - Create, edit, delete domains
  - Manage email accounts with passwords and quotas
  - Configure email aliases and forwarding

### Quick Start

1. **Add a Domain**:
   - Navigate to `https://your-server/admin`
   - Click "Domains" → "Add Domain"
   - Enter domain name (e.g., `example.com`)
   - Check "Auto-generate DKIM keys" for automatic DKIM setup
   - Click "Create Domain"

2. **Configure DNS**:
   - After creating the domain, click the DKIM link
   - Copy the DNS TXT record shown
   - Add it to your DNS provider

3. **Create Email Account**:
   - Navigate to "Email Accounts" → "Add Email Account"
   - Select domain, enter username and password
   - Set quota (0 = unlimited)
   - Click "Create Account"

4. **Set Up Alias** (Optional):
   - Navigate to "Aliases" → "Add Alias"
   - Set source (e.g., `info@example.com`)
   - Set destination (e.g., `admin@example.com`)
   - Click "Create Alias"

### Reverse Proxy Configuration

The Nginx reverse proxy provides:
- **Automatic HTTPS redirect** - All HTTP traffic redirected to HTTPS
- **Path-based routing** - `/admin`, `/admin/reports`, `/pixel` routes
- **Self-signed certificates** - Generated automatically during build
- **Security headers** - X-Frame-Options, X-Content-Type-Options, X-XSS-Protection

### Database Location

The admin panel stores all data in:
```
data/admin/database.sqlite
```

Regular backups of this file are recommended.

### Port Configuration

All ports are standardized and configurable via environment variables. See [PORT_CONFIGURATION.md](PORT_CONFIGURATION.md) for detailed documentation.

**External Ports** (exposed to host):
- `80, 443` - HTTP/HTTPS (Reverse Proxy)
- `25, 587, 465` - SMTP (Postfix)
- `143, 993` - IMAP (Dovecot)
- `110, 995` - POP3 (Dovecot)

**Internal Ports** (Docker network only):
- `8891` - OpenDKIM
- `8892` - PixelMilter
- `8443, 8444` - PixelServer
- `24, 12345` - Dovecot Internal
- `80` - Admin Panel Internal

---

## Security notes

- Replace self‑signed TLS cert with a real one when ready (overwrite in data/ssl and `docker compose restart mail dovecot`)
- Never commit .env, keys, or mail data
- Consider firewalling 25/587/993 as appropriate

---

## Troubleshooting

For detailed troubleshooting steps, diagnostic commands, and common issues, please refer to [TROUBLESHOOTING.md](TROUBLESHOOTING.md).