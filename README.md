# Mailserver

Single-container mail server with built-in admin dashboard.

Alpine + Postfix + Dovecot + OpenDKIM + Rust.

![Dashboard Screenshot](https://github.com/user-attachments/assets/da6ee91d-dfc2-4622-98fd-26622215a500)

## Architecture

```mermaid
graph LR
    Internet((Internet))

    subgraph Docker Container
        Supervisor[Supervisord]
        Admin[Rust Admin Dashboard :8080]
        Filter[Content Filter]
        Postfix[Postfix SMTP :25/587/465]
        Dovecot[Dovecot IMAP/POP3 :143/993/110/995]
        OpenDKIM[OpenDKIM]
        SQLite[(SQLite DB)]

        Supervisor --> Admin
        Supervisor --> Postfix
        Supervisor --> Dovecot
        Supervisor --> OpenDKIM

        Admin -->|read/write| SQLite
        Filter -->|tracking queries & inserts| SQLite
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

    SQLite --- DB
    Dovecot --- Mail
    Postfix --- SSL
    OpenDKIM --- DKIM
```

## Email Flow

```mermaid
sequenceDiagram
    participant Sender as Sender (Internet)
    participant Postfix
    participant Filter as Content Filter
    participant SQLite as SQLite DB
    participant OpenDKIM
    participant Dovecot
    participant Recipient as Recipient (Mailbox)

    Note over Sender,Recipient: Inbound Email
    Sender->>Postfix: SMTP :25
    Postfix->>Filter: pipe via pixelfilter
    Filter->>SQLite: check tracking_enabled for alias
    alt Tracking enabled
        Filter->>SQLite: insert tracked_message
        Filter->>Filter: inject tracking pixel into HTML body
    end
    Filter->>Postfix: reinject via SMTP :10025
    Postfix->>Dovecot: LMTP :24
    Dovecot->>Recipient: store in Maildir

    Note over Sender,Recipient: Outbound Email
    Sender->>Postfix: SMTP :587 (authenticated)
    Postfix->>OpenDKIM: DKIM sign (milter :8891)
    OpenDKIM-->>Postfix: signed message
    Postfix->>Recipient: deliver to remote MTA

    Note over Sender,Recipient: Tracking Pixel Open
    Sender->>Postfix: (later) recipient opens email
    Recipient->>SQLite: GET /pixel?id=... → record pixel_open
```

## Database Schema (ERD)

```mermaid
erDiagram
    admins {
        INTEGER id PK
        TEXT username UK
        TEXT password_hash
        TEXT totp_secret
        INTEGER totp_enabled
        TEXT created_at
        TEXT updated_at
    }

    domains {
        INTEGER id PK
        TEXT domain UK
        INTEGER active
        TEXT dkim_selector
        TEXT dkim_private_key
        TEXT dkim_public_key
        TEXT created_at
        TEXT updated_at
    }

    accounts {
        INTEGER id PK
        INTEGER domain_id FK
        TEXT username
        TEXT password_hash
        TEXT name
        INTEGER active
        INTEGER quota
        TEXT created_at
        TEXT updated_at
    }

    aliases {
        INTEGER id PK
        INTEGER domain_id FK
        TEXT source
        TEXT destination
        INTEGER active
        INTEGER tracking_enabled
        TEXT created_at
        TEXT updated_at
    }

    tracked_messages {
        INTEGER id PK
        TEXT message_id UK
        TEXT sender
        TEXT recipient
        TEXT subject
        INTEGER alias_id FK
        TEXT created_at
    }

    pixel_opens {
        INTEGER id PK
        TEXT message_id FK
        TEXT client_ip
        TEXT user_agent
        TEXT opened_at
    }

    domains ||--o{ accounts : "has"
    domains ||--o{ aliases : "has"
    aliases ||--o{ tracked_messages : "triggers"
    tracked_messages ||--o{ pixel_opens : "records"
```

## Use Cases

```mermaid
graph TB
    AdminUser((Admin))
    EmailUser((Email User))
    RemoteSender((Remote Sender))

    subgraph Admin Dashboard
        ManageDomains[Manage Domains]
        GenerateDKIM[Generate DKIM Keys]
        ViewDNS[View DNS Records]
        ManageAccounts[Manage Accounts]
        ManageAliases[Manage Aliases]
        ViewTracking[View Tracking Reports]
        ChangePassword[Change Password]
        Configure2FA[Enable / Disable 2FA]
    end

    subgraph Mail Services
        SendEmail[Send Email via SMTP]
        ReceiveEmail[Receive Email]
        ReadEmail[Read Email via IMAP / POP3]
    end

    AdminUser --> ManageDomains
    AdminUser --> GenerateDKIM
    AdminUser --> ViewDNS
    AdminUser --> ManageAccounts
    AdminUser --> ManageAliases
    AdminUser --> ViewTracking
    AdminUser --> ChangePassword
    AdminUser --> Configure2FA

    EmailUser --> SendEmail
    EmailUser --> ReadEmail
    RemoteSender --> ReceiveEmail
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

Open `http://your-host:443` for the admin dashboard.

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
