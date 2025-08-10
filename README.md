# 📧 Dockerized Postfix + OpenDKIM Mailserver

A production‑ready, self‑contained mailserver with:
- Postfix SMTP (25, 587) + Submission AUTH
- OpenDKIM signing
- Self‑signed or user‑supplied TLS certs
- Persistent configuration and data
- Catch‑all routing to your submission user

---

## 🚀 Quick Start

```bash
# Clone and enter
git clone https://example.com/your-repo.git mailserver
cd mailserver

# Configure your environment
cp .env.example .env

# One‑time setup (certs, containers up, test)
apt-get install make build-essential
make install

# Generate initial TLS certs
make certs

# Bring up containers
docker compose up -d

# Test server and TLS/AUTH
make test

# Send test mail (set TO=recipient)
make send TO=you@example.net
