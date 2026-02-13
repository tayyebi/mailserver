# Mailserver

Single-container mail server with built-in admin dashboard.

Alpine + Postfix + Dovecot + OpenDKIM + Rust.

![Dashboard Screenshot](https://github.com/user-attachments/assets/da6ee91d-dfc2-4622-98fd-26622215a500)

## Architecture

```mermaid
graph LR
    Internet((Internet))

    subgraph Docker Container
        Admin[Rust Admin Dashboard :8080]
        Postfix[Postfix SMTP :25/587/465]
        Dovecot[Dovecot IMAP/POP3 :143/993/110/995]
        OpenDKIM[OpenDKIM]
        Supervisor[Supervisord]
        SQLite[(SQLite DB)]

        Supervisor --> Admin
        Supervisor --> Postfix
        Supervisor --> Dovecot
        Supervisor --> OpenDKIM

        Admin --> SQLite
        Postfix --> OpenDKIM
        Postfix --> Dovecot
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

    Dovecot --- Mail
    Admin --- DB
    Postfix --- SSL
    OpenDKIM --- DKIM
```

## Quick Start

**Docker run (one-liner with persistent storage):**

```
docker run -d --name mailserver \
  -p 25:25 -p 587:587 -p 465:465 -p 143:143 -p 993:993 -p 110:110 -p 995:995 -p 443:8080 \
  -v maildata:/data \
  -e HOSTNAME=mail.example.com \
  ghcr.io/tayyebi/mailserver:main
```

**Docker Compose:**

```
cp .env.example .env
docker compose up -d
```

Open `https://your-host:443` for the admin dashboard.

## Default Admin

- **Username:** `admin`
- **Password:** `admin`

Change the password immediately after first login via Settings.

## Authentication

The admin dashboard uses HTTP Basic Authentication (browser prompt).
When 2FA is enabled, append your 6-digit TOTP code to your password.

Example: if password is `secret` and TOTP code is `123456`, enter `secret123456`.

## Configuration

Ports and hostname are set in `.env`. Everything else is managed from the admin dashboard:

- **Domains** — add mail domains, generate DKIM keys, view DNS records
- **Accounts** — create email accounts with passwords and quotas
- **Aliases** — set up email forwarding with per-alias tracking toggle
- **Tracking** — view email open tracking reports
- **Settings** — change admin password, enable/disable 2FA

## Data

All persistent data is stored in the `maildata` Docker volume:

- `/data/ssl/` — TLS certificates (auto-generated self-signed)
- `/data/dkim/` — DKIM signing keys
- `/data/mail/` — mailboxes (Maildir format)
- `/data/db/` — SQLite database

## DNS

Required DNS records for each domain are shown in the admin dashboard under Domains → DNS.
