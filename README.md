<div align="center">

# 📬 Mailserver

**A fully self-hosted mail server in a single Docker container.**

Send, receive, and manage email — with a sleek web admin panel, built-in webmail, open tracking, fail2ban protection, and DKIM signing. No complex setup. No third-party dependencies.

[![Docker Image](https://img.shields.io/badge/ghcr.io-tayyebi%2Fmailserver-blue?logo=docker)](https://ghcr.io/tayyebi/mailserver)
[![License](https://img.shields.io/github/license/tayyebi/mailserver)](LICENSE)

> **Less moving parts. Less failure.**

Alpine · Postfix · Dovecot · OpenDKIM · Rust · PostgreSQL — all in one container.


</div>

---

## ✨ Features

| Feature | Description |
|---|---|
| 📋 **Admin Dashboard** | Clean web UI to manage every aspect of your mail server |
| 🌐 **Domain Management** | Add unlimited mail domains with one-click DKIM key generation |
| 👤 **User Accounts** | Create mailboxes with passwords and storage quotas |
| 🔀 **Aliases & Catch-all** | Forward addresses, wildcards (`*@domain.com`), and routing rules |
| 📤 **Forwarding** | Forward mail from local addresses to any external destination, with optional local copy |
| 📡 **Open Tracking** | Pixel-based email open tracking with per-message reports |
| 🌐 **Built-in Webmail** | Read, compose, and manage email directly from your browser |
| 🔒 **Fail2ban Protection** | Auto-ban IPs on repeated auth failures; manage whitelist & blacklist |
| 🛡️ **2FA (TOTP)** | Two-factor authentication for the admin panel |
| 📦 **Queue Management** | View and flush the Postfix mail queue from the dashboard |
| 🗑️ **Unsubscribe Management** | Track and manage unsubscribe requests |
| 🔍 **DNSBL / Spam Blocking** | DNS block-list management integrated with Postfix |
| 📄 **DNS Runbook** | Per-domain DNS record viewer with SPF, DKIM, DMARC guidance |
| 📊 **DMARC Reports** | Designate inboxes to receive DMARC aggregate reports and visualize results |
| 🔁 **Outbound Relays** | Route outbound mail through external SMTP servers, assignable per domain, account, or alias |
| 🔔 **Webhook Notifications** | Send HTTP webhooks on processed outbound emails |
| ⚙️ **Config Viewer** | Inspect live Postfix/Dovecot/OpenDKIM configs from the UI |
| 📁 **WebDAV File Storage** | Per-account WebDAV server at `/dav/{email}/` for file storage and sharing via FileLink |
| 📅 **CalDAV Calendar Server** | Per-account CalDAV server at `/caldav/{email}/` for calendar sync with any CalDAV client |
| 🤖 **MCP API** | Model Context Protocol endpoint for AI assistant integration (list/read/send/delete email) |

---

## 🚀 Quick Start

### Option A — Docker Compose (recommended)

Docker Compose starts Mailserver together with a PostgreSQL database automatically:

```bash
cp .env.example .env
# Edit .env to set your HOSTNAME and other settings
docker compose up -d
```

Then open **http://your-server:8080** in your browser.

### Option B — Docker Run (bring your own PostgreSQL)

If you already have a PostgreSQL instance, you can run the container directly:

```bash
docker run -d --name mailserver \
  -p 25:25 -p 587:587 -p 465:465 -p 2525:2525 \
  -p 143:143 -p 993:993 -p 110:110 -p 995:995 \
  -p 8080:8080 \
  -v maildata:/data \
  -e HOSTNAME=mail.example.com \
  -e DATABASE_URL=postgres://mailserver:mailserver@your-pg-host/mailserver \
  ghcr.io/tayyebi/mailserver:main
```

---

## 🔑 First Login

| Field | Value |
|---|---|
| **Username** | `admin` |
| **Password** | `admin` |

> ⚠️ **Change your password immediately** after first login via **Settings**.

### Two-Factor Authentication (2FA)

Enable TOTP-based 2FA from the Settings page. Once enabled, append your 6-digit code to your password at login.

**Example:** password `secret` + TOTP `123456` → enter `secret123456`

---

## 🌐 Admin Dashboard Walkthrough

### Domains

Add your mail domains, generate DKIM signing keys with one click, and get a ready-to-use DNS runbook showing every record you need (MX, SPF, DKIM, DMARC, PTR).


### Accounts

Create email accounts for your users. Set display names, passwords, and per-account storage quotas.


### Aliases & Catch-all

Create forwarding rules between addresses. Use `*@yourdomain.com` as a catch-all to capture mail sent to any address on the domain. Toggle open tracking per alias.

### Forwarding

Set up rules to forward mail from a local address to any external email address. Optionally keep a local copy in the original mailbox. Useful for redirecting mail to third-party inboxes without changing the sender's experience.

### Open Tracking

When tracking is enabled on an alias, outgoing emails get a tiny invisible tracking pixel injected into the HTML body. Every time the recipient opens the email, a record is created. View detailed per-message open reports from the **Tracking** section.


### Webmail

A lightweight webmail client is built right into the admin panel. Browse folders, read messages, compose new emails (with CC, BCC, Reply-To, priority, and custom headers), and delete messages — all without leaving the browser.


### Fail2ban

Mailserver includes a built-in fail2ban system that monitors Postfix and Dovecot logs for repeated authentication failures on SMTP, IMAP, and POP3. Offending IPs are automatically banned. You can:

- Configure thresholds and ban duration per service
- Manually ban or unban individual IPs or CIDR ranges
- Maintain a permanent whitelist and blacklist
- Review a full audit log of all ban/unban events


### Queue

Inspect the live Postfix mail queue and flush stuck messages directly from the admin panel — no SSH required.

### DMARC Reports

Designate one or more mailboxes as DMARC report inboxes. The dashboard automatically parses incoming DMARC aggregate reports and lets you visualize pass/fail results, sending sources, and policy dispositions per domain.

### DNS Check

Per-domain DNS health checker with individual shortcut links for each record type. Catch delivery problems before they affect your users.

### Config Viewer

Inspect the live Postfix, Dovecot, and OpenDKIM configuration files generated from your database — useful for debugging.

### Outbound Relays

Configure external SMTP relays to route outbound mail through third-party providers (e.g. SendGrid, SES, or a corporate relay). Relays can be assigned globally or scoped to a specific domain, account, or alias, with support for PLAIN and LOGIN authentication.

### WebDAV File Storage

Each mail account gets a personal WebDAV drive at `/dav/{email}/`. Users can mount it in their OS file manager, upload/download files, and share individual files via one-time FileLink download URLs — all authenticated with their mail credentials.

### CalDAV Calendar Server

A built-in CalDAV server at `/caldav/{email}/` lets users sync calendars using any CalDAV-compatible client (Thunderbird, Apple Calendar, DAVx⁵ on Android, etc.). Calendars are created from the admin panel and are scoped per mail account.

### MCP API (AI Assistant Integration)

A [Model Context Protocol](https://modelcontextprotocol.io/) endpoint at `POST /mcp` exposes mail operations to AI assistants and automation tools. Supported tools: `list_accounts`, `list_emails`, `read_email`, `send_email`, `delete_email`. Authentication uses the same admin credentials.

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
| `8080` | HTTP | Admin dashboard & webmail |

---

## ⚙️ Configuration

All settings are managed from the admin dashboard. The only file you need to edit before starting is `.env`:

| Variable | Default | Description |
|---|---|---|
| `HOSTNAME` | `mail.example.com` | Fully-qualified domain name of the mail server |
| `HTTP_PORT` | `8080` | Admin dashboard port |
| `SMTP_PORT` | `25` | Inbound SMTP port |
| `SUBMISSION_PORT` | `587` | Submission port |
| `DATABASE_URL` | `postgres://mailserver:mailserver@localhost/mailserver` | PostgreSQL connection string |
| `SEED_USER` | `admin` | Initial admin username |
| `SEED_PASS` | `admin` | Initial admin password |
| `TZ` | `UTC` | Container timezone |

---

## 💾 Persistent Data

All mail data is stored in the `maildata` Docker volume mounted at `/data`:

| Path | Contents |
|---|---|
| `/data/ssl/` | TLS certificates (auto-generated self-signed on first start) |
| `/data/dkim/` | DKIM signing keys |
| `/data/mail/` | User mailboxes (Maildir format) |

The PostgreSQL database (accounts, domains, aliases, tracking data) is required by the mail server. When using Docker Compose, it runs in a separate `db` container with its data stored in the `maildb` volume. When running standalone, point `DATABASE_URL` to your own PostgreSQL instance.

---

## 🌍 DNS Setup

After adding a domain in the admin panel, go to **Domains → DNS** to get the exact DNS records you need to publish:

- **MX** — points incoming mail to your server
- **SPF** — authorizes your server to send mail for the domain
- **DKIM** — cryptographic signature for outbound mail (key generated in the dashboard)
- **DMARC** — policy for handling SPF/DKIM failures
- **PTR** — reverse DNS (set at your VPS provider)

The dashboard shows copy-pasteable values for every record.

---

## 🏗️ Architecture

```mermaid
graph LR
    Internet((Internet))

    subgraph Docker Container
        Admin[Rust Admin Dashboard :8080]
        Filter[Content Filter + Footer Injector]
        Postfix[Postfix SMTP :25/587/465]
        Dovecot[Dovecot IMAP/POP3 :143/993/110/995]
        OpenDKIM[OpenDKIM]
        Postgres[(PostgreSQL DB)]

        Admin -->|read/write| Postgres
        Filter -->|tracking & footer lookups| Postgres
        Admin -->|generate configs from DB| Postfix
        Admin -->|generate passwd from DB| Dovecot
        Admin -->|generate key tables from DB| OpenDKIM
        Postfix -->|DKIM signing| OpenDKIM
        Postfix -->|LMTP delivery| Dovecot
        Postfix -->|pipe emails| Filter
        Filter -->|reinject via SMTP :10025| Postfix
    end

    subgraph Persistent Volume /data
        SSL["/data/ssl"]
        DKIM["/data/dkim"]
        Mail["/data/mail"]
        DB["/data/db"]
    end

    Internet -->|SMTP| Postfix
    Internet -->|IMAP/POP3| Dovecot
    Internet -->|HTTPS| Admin

    Postgres --- DB
    Dovecot --- Mail
    Postfix --- SSL
    OpenDKIM --- DKIM
```

## 📨 Email Flow

```mermaid
sequenceDiagram
    participant Sender as Sender (Internet)
    participant Postfix
    participant Filter as Content Filter
    participant Postgres as PostgreSQL DB
    participant OpenDKIM
    participant Dovecot
    participant Recipient as Recipient (Mailbox)

    Note over Sender,Recipient: Inbound Email
    Sender->>Postfix: SMTP :25
    Postfix->>Dovecot: LMTP :24
    Dovecot->>Recipient: store in Maildir

    Note over Sender,Recipient: Outbound Email
    Sender->>Postfix: SMTP :587 (authenticated)
    Postfix->>Filter: pipe via pixelfilter
    Filter->>Postgres: lookup tracking + footer_html
    alt Footer configured
        Filter->>Filter: inject domain footer (HTML/plain text)
    end
    alt Tracking enabled
        Filter->>Postgres: insert tracked_message
        Filter->>Filter: inject tracking pixel into HTML body
    end
    Filter->>Postfix: reinject via SMTP :10025
    Postfix->>OpenDKIM: DKIM sign (milter :8891)
    OpenDKIM-->>Postfix: signed message
    Postfix->>Recipient: deliver to remote MTA

    Note over Sender,Recipient: Tracking Pixel Open
    Sender->>Postfix: (later) recipient opens email
    Recipient->>Postgres: GET /pixel?id=... → record pixel_open
```
