<div align="center">

# 📬 Mailserver

**A fully self-hosted mail server — single Docker container or single binary.**

Send, receive, and manage email with a sleek web admin panel, built-in webmail, open tracking, fail2ban protection, DKIM signing, CalDAV/CardDAV, and more. No complex setup. No third-party dependencies.

[![Docker Image](https://img.shields.io/badge/ghcr.io-tayyebi%2Fmailserver-blue?logo=docker)](https://ghcr.io/tayyebi/mailserver)
[![License](https://img.shields.io/github/license/tayyebi/mailserver)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)](https://www.rust-lang.org/)

> **Less moving parts. Less failure.**

Alpine · Postfix · Dovecot · OpenDKIM · Rust · PostgreSQL

</div>

---

## Table of Contents

- [Features](#-features)
- [Installation](#-installation)
  - [Method 1 — Docker Compose (recommended)](#method-1--docker-compose-recommended)
  - [Method 2 — Docker Run](#method-2--docker-run)
  - [Method 3 — Docker Image from Release](#method-3--docker-image-from-release)
  - [Method 4 — Single Binary (bare metal)](#method-4--single-binary-bare-metal)
  - [Method 5 — Auto-Provisioning (SSH)](#method-5--auto-provisioning-ssh)
  - [Method 6 — Kubernetes Manifest (K8s providers)](#method-6--kubernetes-manifest-k8s-providers)
- [First Login](#-first-login)
- [Admin Dashboard](#-admin-dashboard)
- [Port Reference](#-port-reference)
- [Configuration](#️-configuration)
- [Persistent Data](#-persistent-data)
- [DNS Setup](#-dns-setup)
- [Architecture](#️-architecture)
- [Email Flow](#-email-flow)

---

## ✨ Features

| Feature | Description |
|---|---|
| 🛠️ **Auto-Provisioning** | One-command SSH deployment to any Linux VPS — idempotent, verbose, zero credential storage |
| 📋 **Admin Dashboard** | Clean web UI to manage every aspect of your mail server |
| 🌐 **Domain Management** | Add unlimited mail domains with one-click DKIM key generation and per-domain BIMI logo support |
| 👤 **User Accounts** | Create mailboxes with display names, passwords, and per-account storage quotas |
| 🔀 **Aliases & Catch-all** | Forward addresses, wildcards (`*@domain.com`), and flexible routing rules |
| 📤 **Forwarding** | Forward mail from local addresses to any external destination, with optional local copy |
| 🦶 **Email Footers** | Automatically inject branded HTML and plain-text footers into outbound mail per domain |
| 📡 **Open Tracking** | Pixel-based email open tracking with per-message reports and conditional rules |
| ⏱️ **Rate Limiting** | Configurable per-account/per-domain outbound sending rate limits with conditional rules |
| 🌐 **Built-in Webmail** | Read, compose, and manage email directly from your browser with IMAP IDLE push |
| 🔒 **Fail2ban Protection** | Auto-ban IPs on repeated auth failures; manage whitelist & blacklist with full audit log |
| 🛡️ **2FA (TOTP)** | Two-factor authentication for the admin panel |
| 📦 **Queue Management** | View and flush the Postfix mail queue from the dashboard |
| 🗑️ **Unsubscribe Management** | Track and manage unsubscribe requests |
| 🔍 **DNSBL / Spam Blocking** | DNS block-list management integrated with Postfix |
| 📄 **DNS Runbook** | Per-domain DNS record viewer with SPF, DKIM, DMARC, BIMI guidance |
| 📊 **DMARC Reports** | Designate inboxes to receive DMARC aggregate reports and visualize pass/fail results |
| 🔁 **Outbound Relays** | Route outbound mail through external SMTP servers, assignable per domain, account, or alias |
| 🔔 **Webhook Notifications** | Send HTTP webhooks on processed outbound emails |
| ⚙️ **Config Viewer** | Inspect live Postfix/Dovecot/OpenDKIM configs from the UI |
| 📁 **WebDAV File Storage** | Per-account WebDAV server at `/dav/{email}/` for file storage and FileLink sharing |
| 📅 **CalDAV Calendar Server** | Per-account CalDAV server at `/caldav/{email}/` for calendar sync with any CalDAV client |
| 📇 **CardDAV Contact Server** | Per-account CardDAV server at `/carddav/{email}/` for contact sync with any CardDAV client |
| 🖼️ **BIMI Support** | Serve per-domain SVG brand logos at `/bimi/{domain}/logo.svg` for supporting mail clients |
| 🤖 **MCP API** | Model Context Protocol endpoint for AI assistant integration (list/read/send/delete email) |
| 📡 **REST & SOAP APIs** | Programmatic access to mail operations via REST and SOAP endpoints |
| 📝 **Self-Registration** | Optional user self-registration portal for invite-based account creation |
| 🚨 **Abuse Reporting** | Built-in abuse complaint handling and reporting |

---

## 🚀 Installation

### Method 1 — Docker Compose (recommended)

The simplest path. Docker Compose starts the mail server **and** a PostgreSQL database together with a single command. Everything — TLS certificates, DKIM keys, Postfix/Dovecot/OpenDKIM configs — is generated automatically on first start.

**Prerequisites:** Docker Engine 24+ and Docker Compose v2.

**Step 1 — Clone the repository and create your environment file**

```bash
git clone https://github.com/tayyebi/mailserver.git
cd mailserver
cp .env.example .env
```

**Step 2 — Set your hostname**

Open `.env` and set `HOSTNAME` to the fully-qualified domain name you'll use for mail (e.g. `mail.example.com`). Change `SEED_PASS` while you're there.

```bash
# .env (minimum required change)
HOSTNAME=mail.example.com
SEED_PASS=changeme
```

**Step 3 — Start the stack**

```bash
docker compose up -d
```

This starts two containers:
- `db` — PostgreSQL 16 (data stored in the `maildb` volume)
- `mailserver` — the mail server (data stored in the `maildata` volume)

**Step 4 — Open the admin dashboard**

```
http://your-server-ip:8080
```

Login with `admin` / `changeme` (or whatever you set in `SEED_PASS`).

**Upgrading**

```bash
docker compose pull
docker compose up -d
```

---

### Method 2 — Docker Run

Use this if you already have a PostgreSQL instance you want to reuse.

**Prerequisites:** Docker Engine 24+ and a running PostgreSQL instance.

**Step 1 — Create the database**

```sql
CREATE USER mailserver WITH PASSWORD 'strongpassword';
CREATE DATABASE mailserver OWNER mailserver;
```

**Step 2 — Run the container**

```bash
docker run -d --name mailserver \
  --restart unless-stopped \
  -p 25:25 -p 587:587 -p 465:465 -p 2525:2525 \
  -p 143:143 -p 993:993 -p 110:110 -p 995:995 \
  -p 8080:8080 \
  -v maildata:/data \
  -e HOSTNAME=mail.example.com \
  -e DATABASE_URL=postgres://mailserver:strongpassword@your-pg-host/mailserver \
  -e SEED_PASS=changeme \
  -e TZ=UTC \
  ghcr.io/tayyebi/mailserver:main
```

**Step 3 — Open the admin dashboard**

```
http://your-server-ip:8080
```

**To put the admin panel behind HTTPS**, place Nginx or Caddy in front and proxy to port 8080. The mail ports (25, 587, 465, 143, 993, etc.) connect directly.

---

### Method 3 — Docker Image from Release

Download a pre-built Docker image from the [Releases page](https://github.com/tayyebi/mailserver/releases) — no local build needed.

#### Option A — Run directly on this machine

**Step 1 — Download the tarball**

```bash
curl -L https://github.com/tayyebi/mailserver/releases/latest/download/mailserver-docker.tar \
  -o mailserver-docker.tar
```

**Step 2 — Load the image**

The release tarball is created with `docker save`, so use `docker load` to preserve the entrypoint and all image metadata:

```bash
docker load -i mailserver-docker.tar
```

After loading, tag the image:

```bash
docker tag <image-id> mailserver:latest
```

(Find `<image-id>` from the `Loaded image ID` line printed by `docker load`, then replace the `sha256:` prefix with the full SHA.)

**Step 3 — Run the container**

```bash
docker run -d --name mailserver \
  --restart unless-stopped \
  -p 25:25 -p 587:587 -p 465:465 -p 2525:2525 \
  -p 143:143 -p 993:993 -p 110:110 -p 995:995 \
  -p 8080:8080 \
  -v maildata:/data \
  -e HOSTNAME=mail.example.com \
  -e DATABASE_URL=postgres://mailserver:strongpassword@your-pg-host/mailserver \
  -e SEED_PASS=changeme \
  -e TZ=UTC \
  mailserver:latest
```

> ⚠️ **Do not use `docker import`** — it creates a filesystem-only image and strips the ENTRYPOINT, resulting in `Error response from daemon: no command specified` at runtime. Always use `docker load` for tarballs created with `docker save`.

**Step 4 — Open the admin dashboard**

```
http://your-server-ip:8080
```

#### Option B — Upload to a remote server (air-gapped / restricted network)

Use this when the target server cannot reach GitHub directly (e.g. behind a firewall or in a restricted region).

**Step 1** — On your local machine, download the tarball from the [Releases page](https://github.com/tayyebi/mailserver/releases):

```bash
curl -L https://github.com/tayyebi/mailserver/releases/latest/download/mailserver-docker.tar \
  -o mailserver-docker.tar
```

**Step 2** — Upload the tarball to the remote server via `scp`:

```bash
scp mailserver-docker.tar user@your-server:/tmp/mailserver-docker.tar
```

**Step 3** — SSH into the remote server and load the image:

```bash
ssh user@your-server
docker load -i /tmp/mailserver-docker.tar
```

Note the `Loaded image ID: sha256:...` output — you will need the image ID to tag it.

**Step 4** — Tag the image with a friendly name:

```bash
docker tag <image-id> ghcr.io/tayyebi/mailserver:latest
```

**Step 5** — Create a `docker-compose.yml` and `.env` file (see [Method 1](#method-1--docker-compose-recommended) for the full compose template) or run directly:

```bash
docker run -d --name mailserver \
  --restart unless-stopped \
  -p 25:25 -p 587:587 -p 465:465 -p 2525:2525 \
  -p 143:143 -p 993:993 -p 110:110 -p 995:995 \
  -p 8080:8080 \
  -v maildata:/data \
  -e HOSTNAME=mail.example.com \
  -e DATABASE_URL=postgres://mailserver:strongpassword@your-pg-host/mailserver \
  -e SEED_PASS=changeme \
  -e TZ=UTC \
  ghcr.io/tayyebi/mailserver:latest
```

**Step 6** — Clean up the temporary tarball:

```bash
rm /tmp/mailserver-docker.tar
```

---

### Method 4 — Single Binary (bare metal)

The `mailserver` binary is fully self-contained: config templates, database migrations, and static assets are all compiled in. You only need to install the system mail services it manages.

**Supported distros:** Debian/Ubuntu (tested), Alpine, RHEL/CentOS (via `dnf`/`yum`).

#### Step 1 — Install system dependencies

**Debian / Ubuntu:**

```bash
apt-get update
apt-get install -y \
  postfix postfix-pcre \
  dovecot-core dovecot-imapd dovecot-pop3d dovecot-lmtpd \
  opendkim opendkim-tools \
  openssl curl \
  postgresql postgresql-client
```

When the Postfix installer prompts for a mail type, choose **"No configuration"** — the binary generates the config itself.

**Alpine:**

```bash
apk add --no-cache \
  postfix dovecot dovecot-lmtpd dovecot-pop3d \
  opendkim opendkim-utils \
  openssl curl \
  postgresql16
```

#### Step 2 — Set up PostgreSQL

```bash
# Start PostgreSQL (Debian/Ubuntu)
systemctl enable --now postgresql

# Create the database and user
sudo -u postgres psql <<'SQL'
CREATE USER mailserver WITH PASSWORD 'strongpassword';
CREATE DATABASE mailserver OWNER mailserver;
SQL
```

#### Step 3 — Download the binary

Download the latest binary from the [Releases page](https://github.com/tayyebi/mailserver/releases) or pull it from the container image:

```bash
# Option A — from GitHub Releases
curl -L https://github.com/tayyebi/mailserver/releases/latest/download/mailserver \
  -o /usr/local/bin/mailserver
chmod +x /usr/local/bin/mailserver

# Option B — extract from the Docker image (always matches main branch)
docker create --name tmp ghcr.io/tayyebi/mailserver:main
docker cp tmp:/usr/local/bin/mailserver /usr/local/bin/mailserver
docker rm tmp
chmod +x /usr/local/bin/mailserver
```

#### Step 4 — Create the environment file

```bash
mkdir -p /etc/mailserver

cat > /etc/mailserver/env <<'EOF'
HOSTNAME=mail.example.com
DATABASE_URL=postgres://mailserver:strongpassword@localhost/mailserver
ADMIN_PORT=8080
SEED_USER=admin
SEED_PASS=changeme
TZ=UTC
EOF

chmod 600 /etc/mailserver/env
```

#### Step 5 — Create system users and directories

```bash
# vmail user for Dovecot mailbox ownership
useradd -r -s /sbin/nologin -d /dev/null vmail 2>/dev/null || true

# Required directories
mkdir -p /data/ssl /data/dkim /data/mail
chown -R vmail:vmail /data/mail
```

#### Step 6 — Run initial setup

```bash
set -a; source /etc/mailserver/env; set +a

# Generate TLS certificates (skipped if /data/ssl/cert.pem already exists)
mailserver gencerts

# Seed the admin user into the database
mailserver seed

# Generate Postfix / Dovecot / OpenDKIM config files
mailserver genconfig
```

#### Step 7 — Verify the generated configuration files

`mailserver genconfig` writes the live mail-service config directly into the system paths used by Postfix, Dovecot, and OpenDKIM. There is no extra "copy templates into `/etc`" step on bare metal.

The main generated files are:

```text
/etc/postfix/main.cf
/etc/postfix/master.cf
/etc/postfix/virtual_domains
/etc/postfix/vmailbox
/etc/postfix/virtual_aliases
/etc/postfix/recipient_bcc
/etc/postfix/sender_login_maps
/etc/postfix/transport_maps
/etc/postfix/sasl_passwd
/etc/dovecot/dovecot.conf
/etc/dovecot/passwd
/etc/opendkim/opendkim.conf
/etc/opendkim/KeyTable
/etc/opendkim/SigningTable
/etc/opendkim/TrustedHosts
```

Postfix map files are also compiled with `postmap` during `genconfig`, and DKIM private keys are written under `/data/dkim`.

You can quickly inspect the generated files before enabling services:

```bash
ls -l /etc/postfix /etc/dovecot /etc/opendkim
postconf -n
```

> **Important:** Treat these files as generated output. If you re-run `mailserver genconfig` (or start the managed service), it will regenerate them in place.

#### Step 8 — Install the systemd service

```bash
cat > /etc/systemd/system/mailserver.service <<'EOF'
[Unit]
Description=Mailserver (Postfix + Dovecot + OpenDKIM)
After=network.target postgresql.service
Wants=network.target

[Service]
Type=simple
EnvironmentFile=/etc/mailserver/env
ExecStartPre=/usr/local/bin/mailserver genconfig
ExecStart=/usr/local/bin/mailserver serve
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable --now mailserver
```

> **Note:** `serve` manages configuration regeneration but does **not** start Postfix/Dovecot/OpenDKIM — those are managed by the OS service manager. The `entrypoint.sh` in the Docker image handles starting all four together; for bare-metal you manage each service separately.

Start the mail services:

```bash
systemctl enable --now dovecot opendkim postfix
```

#### Step 9 — Open the admin dashboard

```
http://your-server-ip:8080
```

#### Resetting the admin password

If you ever lose access:

```bash
RESET_USER=admin RESET_PASS=newpassword mailserver reset-password
```

---

### Method 5 — Auto-Provisioning (SSH)

Spin up a fresh mailserver on **any Linux VPS in one command** — no manual SSH steps, no config files to write by hand. Run this from your local machine.

**Prerequisites:** The `mailserver` binary on your local machine (see [Method 4 — Single Binary](#method-4--single-binary-bare-metal), Step 3).

```bash
mailserver provision \
  --host mail.example.com \
  --user root \
  --key ~/.ssh/id_ed25519
```

The command connects over SSH and idempotently:

1. **Detects the package manager** — `apt-get`, `apk`, `dnf`, or `yum`
2. **Installs system dependencies** — Postfix, Dovecot, OpenDKIM, OpenSSL, PostgreSQL — skipped if already present
3. **Creates system users and directories** — `vmail`, `opendkim`, `/data/…` — skipped if already present
4. **Uploads the current binary** — copies itself to `/usr/local/bin/mailserver`
5. **Runs initial setup** — `gencerts` (skipped if certs exist), `seed`, `genconfig`
6. **Installs the system service** — writes a `systemd` unit (or OpenRC init script on Alpine) — skipped if already installed
7. **Enables and starts the service**

Every step produces verbose log output so you can see exactly what is and isn't being done.

**Options:**

| Flag | Default | Description |
|---|---|---|
| `--host <host>` | *(required)* | Remote hostname or IP address |
| `--port <port>` | `22` | SSH port |
| `--user <user>` | *(required)* | SSH login username |
| `--key <path>` | — | Path to SSH private key (recommended) |
| `--password <pwd>` | — | Password for SSH auth **or** passphrase for an encrypted key |
| `--env-file <path>` | — | Local `.env` file to upload as `/etc/mailserver/env` |

> Credentials are held in memory only — they are never written to disk.

**Examples:**

```bash
# Key-based auth (recommended)
mailserver provision --host mail.example.com --user root --key ~/.ssh/id_ed25519

# Upload an environment file (sets HOSTNAME, DATABASE_URL, SEED_PASS, etc. on the remote)
mailserver provision --host mail.example.com --user root \
  --key ~/.ssh/id_ed25519 --env-file .env.prod

# Password auth
mailserver provision --host mail.example.com --user root --password s3cr3t
```

---

### Method 6 — Kubernetes Manifest (K8s providers)

Use the included manifest file if you deploy on Kubernetes providers (EKS/GKE/AKS, etc.).

It uses the current registry image: `ghcr.io/tayyebi/mailserver:main`.

**Step 1 — Edit required placeholders**

Update these values in `k8s-mailserver-manifest.yaml` before applying:
- `DATABASE_URL`
- `SEED_PASS`
- `HOSTNAME`

**Step 2 — Apply the manifest**

```bash
kubectl apply -f k8s-mailserver-manifest.yaml
```

**Step 3 — Get external endpoint**

```bash
kubectl -n mailserver get svc mailserver
```

Use the service external IP/hostname to access the admin panel on port `8080`.
Route mail ports (25/465/587/143/993/110/995) to the same endpoint.
Also configure DNS records for mail delivery:
- `A`/`AAAA` — resolves your mail host name to the load balancer endpoint
- `MX` — routes domain mail flow to your mail host
- `PTR` — reverse DNS for the public IP (required by many receiving providers)
- `SPF`, `DKIM`, `DMARC` — sender authentication and deliverability protection

---

## 🔑 First Login

| Field | Value |
|---|---|
| **URL** | `http://your-server:8080` |
| **Username** | `admin` (or `SEED_USER`) |
| **Password** | `admin` (or `SEED_PASS`) |

> ⚠️ **Change your password immediately** after first login via **Settings → Password**.

If you lose access, reset the password without a login session:

```bash
RESET_USER=admin RESET_PASS=newpassword mailserver reset-password
```

### Two-Factor Authentication (2FA)

Enable TOTP-based 2FA from the Settings page. Once enabled, append your 6-digit code to your password at login.

**Example:** password `secret` + TOTP `123456` → enter `secret123456`

---

## 🌐 Admin Dashboard

### Domains

Add your mail domains, generate DKIM signing keys with one click, and get a ready-to-use DNS runbook showing every record you need (MX, SPF, DKIM, DMARC, BIMI, PTR). Upload a per-domain SVG logo for BIMI support in compatible mail clients.

### Accounts

Create email accounts for your users. Set display names, passwords, and per-account storage quotas. Each account automatically gets WebDAV, CalDAV, and CardDAV access at the corresponding endpoints.

### Aliases & Catch-all

Create forwarding rules between addresses. Use `*@yourdomain.com` as a catch-all to capture mail sent to any address on the domain. Toggle open tracking and footer injection per alias.

### Forwarding

Set up rules to forward mail from a local address to any external email address. Optionally keep a local copy in the original mailbox.

### Email Footers

Define HTML and plain-text footers that are automatically appended to outbound emails. Rules let you scope footers by sender pattern, domain, or alias.

### Open Tracking

When tracking is enabled on an alias, outgoing emails get a tiny invisible tracking pixel injected into the HTML body. Every time the recipient opens the email, a record is created. View detailed per-message open reports from the **Tracking** section.

### Rate Limiting

Define per-account or per-domain outbound sending rate limits (e.g. max N messages per hour). Conditional rules allow fine-grained control.

### Webmail

A lightweight webmail client built right into the admin panel. Browse folders, read messages, compose new emails (with CC, BCC, Reply-To, priority, and custom headers), and delete messages. Uses IMAP IDLE for real-time push delivery of new messages.

### Fail2ban

Monitors Postfix and Dovecot logs for repeated authentication failures. Offending IPs are automatically banned. Configure thresholds, manage whitelist/blacklist, and review a full audit log.

### Queue

Inspect the live Postfix mail queue and flush stuck messages directly from the admin panel — no SSH required.

### DMARC Reports

Designate one or more mailboxes as DMARC report inboxes. The dashboard automatically parses incoming DMARC aggregate reports and visualizes pass/fail results.

### DNS Check

Per-domain DNS health checker. Catch delivery problems before they affect your users.

### Config Viewer

Inspect the live Postfix, Dovecot, and OpenDKIM configuration files generated from your database.

### Outbound Relays

Configure external SMTP relays to route outbound mail through third-party providers (SendGrid, SES, etc.). Relays can be assigned globally or scoped to a specific domain, account, or alias.

### WebDAV File Storage

Each mail account gets a personal WebDAV drive at `/dav/{email}/`. Users can mount it in their OS file manager and share individual files via one-time FileLink download URLs.

### CalDAV Calendar Server

A built-in CalDAV server at `/caldav/{email}/` for syncing calendars with Thunderbird, Apple Calendar, DAVx⁵ on Android, etc.

### CardDAV Contact Server

A built-in CardDAV server at `/carddav/{email}/` for syncing contacts with Apple Contacts, Thunderbird, DAVx⁵, etc.

### MCP API (AI Assistant Integration)

A [Model Context Protocol](https://modelcontextprotocol.io/) endpoint at `POST /mcp` exposes mail operations to AI assistants. Supported tools: `list_accounts`, `list_emails`, `read_email`, `send_email`, `delete_email`.

---

## 🔌 Port Reference

| Port | Protocol | Purpose |
|------|----------|---------|
| `25` | SMTP | Inbound mail from the Internet |
| `587` | SMTP Submission | Outbound mail (authenticated) |
| `465` | SMTPS | Outbound mail over TLS (authenticated) |
| `2525` | SMTP Alt | Alternative submission port |
| `143` | IMAP | Email retrieval (STARTTLS) |
| `993` | IMAPS | Email retrieval over TLS |
| `110` | POP3 | Email retrieval (STARTTLS) |
| `995` | POP3S | Email retrieval over TLS |
| `8080` | HTTP | Admin dashboard, webmail, WebDAV, CalDAV, CardDAV, APIs |

---

## ⚙️ Configuration

All runtime settings are managed from the admin dashboard. The only file you need to edit before starting is `.env`:

| Variable | Default | Description |
|---|---|---|
| `HOSTNAME` | `mail.example.com` | Fully-qualified domain name of the mail server |
| `ADMIN_PORT` / `HTTP_PORT` | `8080` | Admin dashboard port |
| `SMTP_PORT` | `25` | Inbound SMTP port |
| `SUBMISSION_PORT` | `587` | Submission port |
| `DATABASE_URL` | `postgres://mailserver:mailserver@localhost/mailserver` | PostgreSQL connection string |
| `SEED_USER` | `admin` | Initial admin username (used only on first `seed` run) |
| `SEED_PASS` | `admin` | Initial admin password (used only on first `seed` run) |
| `TZ` | `UTC` | Timezone |

---

## 💾 Persistent Data

All mail data is stored under `/data`:

| Path | Contents |
|---|---|
| `/data/ssl/` | TLS certificates (auto-generated self-signed on first start) |
| `/data/dkim/` | DKIM signing keys (generated per domain from the dashboard) |
| `/data/mail/` | User mailboxes in Maildir format (`/data/mail/{domain}/{user}/Maildir`) |

When using Docker Compose, `/data` is stored in the `maildata` volume. On bare metal, it lives directly on the host. Back up the entire `/data` directory and your PostgreSQL database to preserve all mail and configuration.

---

## 🌍 DNS Setup

After adding a domain in the admin panel, go to **Domains → DNS** to get the exact DNS records you need to publish:

| Record | Purpose |
|---|---|
| **MX** | Points incoming mail to your server |
| **SPF** | Authorizes your server to send mail for the domain |
| **DKIM** | Cryptographic signature for outbound mail (key generated in the dashboard) |
| **DMARC** | Policy for handling SPF/DKIM failures |
| **BIMI** | Brand logo display in supporting mail clients (requires DMARC enforcement) |
| **PTR** | Reverse DNS — set at your VPS provider |

The dashboard shows copy-pasteable values for every record.

---

## 🏗️ Architecture

```mermaid
graph TB
    Internet((Internet))

    subgraph container ["Docker Container / Bare-metal Server"]
        direction TB

        subgraph incoming ["Inbound Path"]
            Postfix["Postfix\nSMTP :25 / :587 / :465"]
            Dovecot["Dovecot\nIMAP :143/:993  POP3 :110/:995"]
            OpenDKIM["OpenDKIM\nDKIM milter :8891"]
        end

        subgraph outbound ["Outbound Pipeline"]
            Filter["Content Filter\n(footer · tracking · rate-limit)"]
        end

        subgraph app ["Rust Application :8080"]
            Admin["Admin Dashboard"]
            Webmail["Webmail (IMAP IDLE)"]
            WebDAV["WebDAV  /dav/"]
            CalDAV["CalDAV  /caldav/"]
            CardDAV["CardDAV  /carddav/"]
            Pixel["Pixel Tracker  /pixel/"]
            MCP["MCP API  /mcp"]
            BIMI["BIMI  /bimi/"]
        end

        Postgres[("PostgreSQL")]
    end

    subgraph volume ["Persistent Data  /data"]
        SSL["/data/ssl"]
        DKIMStore["/data/dkim"]
        Mail["/data/mail"]
    end

    Internet -->|"SMTP :25"| Postfix
    Internet -->|"SMTP :587/:465"| Postfix
    Internet -->|"IMAP/POP3"| Dovecot
    Internet -->|"HTTP :8080"| app

    Postfix -->|"LMTP :24"| Dovecot
    Postfix -->|"pipe (outbound)"| Filter
    Postfix <-->|"DKIM milter"| OpenDKIM
    Filter -->|"reinject :10025"| Postfix
    Filter <-->|"lookups"| Postgres

    Admin -->|"read / write"| Postgres
    Admin -->|"genconfig"| Postfix
    Admin -->|"genconfig"| Dovecot
    Admin -->|"genconfig"| OpenDKIM
    Webmail -->|"IMAP"| Dovecot
    WebDAV -->|"read / write"| Postgres
    CalDAV -->|"read / write"| Postgres
    CardDAV -->|"read / write"| Postgres
    Pixel -->|"record open"| Postgres
    MCP -->|"read / write"| Postgres

    Postfix --- SSL
    OpenDKIM --- DKIMStore
    Dovecot --- Mail
```

---

## 📨 Email Flow

```mermaid
sequenceDiagram
    actor Sender as Sender
    participant Postfix
    participant Filter as Content Filter
    participant Postgres as PostgreSQL
    participant OpenDKIM
    participant Dovecot
    actor Recipient as Recipient

    rect rgb(230, 244, 255)
        Note over Sender,Recipient: Inbound Email
        Sender->>Postfix: SMTP :25
        Postfix->>Dovecot: LMTP :24
        Dovecot->>Recipient: store in Maildir
    end

    rect rgb(230, 255, 235)
        Note over Sender,Recipient: Outbound Email
        Sender->>Postfix: SMTP :587 (authenticated)

        Postfix->>Filter: pipe via pixelfilter

        Filter->>Postgres: check rate limit rules
        alt Rate limit exceeded
            Filter-->>Sender: reject (552)
        end

        Filter->>Postgres: lookup footer_html & tracking config
        alt Footer configured
            Filter->>Filter: inject footer (HTML + plain text)
        end
        alt Open tracking enabled
            Filter->>Postgres: insert tracked_message record
            Filter->>Filter: inject tracking pixel into HTML body
        end

        Filter->>Postfix: reinject via SMTP :10025
        Postfix->>OpenDKIM: DKIM sign (milter :8891)
        OpenDKIM-->>Postfix: signed message
        Postfix->>Recipient: deliver to remote MTA
    end

    rect rgb(255, 250, 230)
        Note over Sender,Recipient: Tracking Pixel Open
        Recipient->>Postfix: (later) recipient opens email
        Recipient->>Postgres: GET /pixel?id=… → record pixel_open
    end
```
