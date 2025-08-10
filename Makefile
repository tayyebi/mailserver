# Mailserver automation Makefile
# MohammadReza's build: persistent, idempotent, and batch‑safe.
# Reads .env for MAIL_HOST/MAIL_DOMAIN and creds.

SHELL := /bin/bash
.ONESHELL:
.SHELLFLAGS := -eu -o pipefail -c
MAKEFLAGS += --no-builtin-rules

ENV_FILE := .env
SCRIPTS  := scripts
DOCKER   := docker compose

include $(ENV_FILE)

# Default help
help:
    @echo "Targets:"
    @echo "  test           Run connectivity and TLS/auth checks"
    @echo "  send           Send test mail via submission"
    @echo "  certs          Generate self-signed certs (if missing)"
    @echo "  certs-force    Force regenerate certs"
    @echo "  restart        Restart mail container(s)"
    @echo "  logs           Tail logs of mail + opendkim"

install:
    @echo ">>> Generating TLS certs (if missing)"
    @$(SCRIPTS)/gen-certs.sh
    @echo ">>> Starting containers"
    @$(DOCKER) up -d
    @echo ">>> Running connectivity checks"
    @$(SCRIPTS)/test-mail.sh
    @echo ">>> Install complete — see 'make send TO=m@tyyi.net' to test delivery"

test:
    @$(SCRIPTS)/test-mail.sh

send:
    @[ -n "$$SUBMISSION_USER" ] && [ -n "$$SUBMISSION_PASS" ] && [ -n "$$TO" ] || \
     (echo "Need SUBMISSION_USER, SUBMISSION_PASS in env and TO=m@tyyi.net" && exit 1)
    @$(SCRIPTS)/test-mail.sh -u "$$SUBMISSION_USER" -p "$$SUBMISSION_PASS" -t "$$TO" -s -v

certs:
    @$(SCRIPTS)/gen-certs.sh

certs-force:
    @$(SCRIPTS)/gen-certs.sh --force

restart:
    @$(DOCKER) restart mail opendkim

logs:
    @$(DOCKER) logs -f mail & $(DOCKER) logs -f opendkim

# Ensure dirs
init-data:
    @mkdir -p data/{mail,dovecot,dovecot-conf,spool,postfix,opendkim/{keys,conf},ssl,postfix/maps}
    @touch data/opendkim/conf/{KeyTable,SigningTable,TrustedHosts}
    @chmod 600 data/opendkim/conf/{KeyTable,SigningTable,TrustedHosts} || true

# Add or update a mailbox user (creates maildir, hashes password)
# Usage: make add-user USER=admin@example.com PASS='secret'
add-user:
    @[ -n "$$USER" ] && [ -n "$$PASS" ] || (echo "Need USER=user@domain and PASS=..." && exit 1)
    @HASH=$$(openssl passwd -6 "$$PASS"); \
    echo "$$USER:{SHA512-CRYPT}$$HASH" >> data/dovecot-conf/users
    @mkdir -p "data/mail/$${USER#*@}/$${USER%@*}"/{cur,new,tmp}
    @echo "User $$USER added."

# Add a domain: creates DKIM keys, updates DKIM tables and Postfix domains
# Usage: make add-domain DOMAIN=example.com
add-domain:
    @[ -n "$$DOMAIN" ] || (echo "Need DOMAIN=example.com" && exit 1)
    @mkdir -p "data/opendkim/keys/$$DOMAIN"
    @if [ ! -f "data/opendkim/keys/$$DOMAIN/default.private" ]; then \
      opendkim-genkey -r -s default -d "$$DOMAIN" -D "data/opendkim/keys/$$DOMAIN"; \
      chmod 600 "data/opendkim/keys/$$DOMAIN"/default.private; \
      echo "DKIM key generated for $$DOMAIN"; \
    else echo "DKIM key exists for $$DOMAIN"; fi
    @grep -q "$$DOMAIN" data/opendkim/conf/KeyTable || \
      echo "default._domainkey.$$DOMAIN $$DOMAIN:default:/keys/$$DOMAIN/default.private" >> data/opendkim/conf/KeyTable
    @grep -q "$$DOMAIN" data/opendkim/conf/SigningTable || \
      echo "*@$$DOMAIN default._domainkey.$$DOMAIN" >> data/opendkim/conf/SigningTable
    @mkdir -p data/postfix
    @touch data/postfix/virtual_domains
    @grep -q "^$$DOMAIN$$" data/postfix/virtual_domains || echo "$$DOMAIN" >> data/postfix/virtual_domains
    @echo "Domain $$DOMAIN added. Publish the TXT record from data/opendkim/keys/$$DOMAIN/default.txt"

reload:
    @docker compose exec opendkim pkill -HUP opendkim || true
    @docker compose exec mail postfix reload || true
    @docker compose exec dovecot dovecot reload || true

.PHONY: help test send certs certs-force restart logs
