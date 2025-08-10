# 📧 Dockerized Postfix + Dovecot + OpenDKIM Mailserver

Self-contained, persistent mail stack:
- Postfix SMTP (25, 587) with DKIM signing
- Dovecot IMAP/IMAPS and SASL auth
- Dovecot LMTP for final delivery to Maildir
- Shared TLS certificates (read-only) for Postfix and Dovecot
- Catch‑all routing to your submission user by default
- Multi‑domain sending/signing with simple Make targets

---

## Prerequisites

- Docker and Docker Compose
- GNU Make

Install GNU Make:
- Debian/Ubuntu: `sudo apt update && sudo apt install make`
- Fedora/RHEL: `sudo dnf install make` (or `sudo yum install make`)
- Arch: `sudo pacman -S make`
- macOS: `brew install make` (use `gmake` if installed as GNU Make)
- Windows: use WSL (then follow Linux), or MSYS2 (`pacman -S make`)

Verify: `make --version`

---

## Install

```bash
# Clone and enter
git clone <your-repo-url> mailserver
cd mailserver

# Create config from template
cp .env.example .env
# Edit: MAIL_DOMAIN, MAIL_HOST, SUBMISSION_USER, SUBMISSION_PASS, TZ

# One‑shot bootstrap (idempotent)
make install
```

`make install` will:
- Ensure data directories exist
- Generate self‑signed TLS certs if missing
- Start opendkim, dovecot, and postfix
- Run health checks

---

## Make targets

- install — bootstrap: init data, certs, up, test
- test — connectivity, STARTTLS, SASL checks
- send — send a test email over submission (requires SUBMISSION_USER, SUBMISSION_PASS, TO)
- certs / certs-force — generate or regenerate TLS certs
- add-user USER=user@domain PASS='secret' — add/update mailbox and create Maildir
- add-domain DOMAIN=example.com — add domain for inbound + DKIM
- reload — reload opendkim, postfix, and dovecot
- restart — restart containers
- logs — tail mail and opendkim logs

Examples:
```bash
make add-domain DOMAIN=example.net
make add-user USER=admin@example.com PASS='StrongPass123!'
make send TO=you@example.net SUBMISSION_USER=admin@example.com SUBMISSION_PASS='StrongPass123!'
```

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

---

## Security notes

- Replace self‑signed TLS cert with a real one when ready (overwrite in data/ssl and `docker compose restart mail dovecot`)
- Never commit .env, keys, or mail data
- Consider firewalling 25/587/993 as appropriate

---