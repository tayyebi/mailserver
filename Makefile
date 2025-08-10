SHELL := /bin/bash
.DEFAULT_GOAL := help

ifneq (,$(wildcard ./.env))
include .env
export
endif

.PHONY: help install test send certs certs-force add-user add-domain reload restart logs

help:
	@echo "Available targets:"
	@echo "  make install							Bootstrap all services and data"
	@echo "  make test							   Run mailserver health checks"
	@echo "  make send TO=addr					   Send test email over submission"
	@echo "  make certs							  Generate TLS certs if missing"
	@echo "  make certs-force						Regenerate TLS certs"
	@echo "  make add-user USER=... PASS=...		 Add/update a mailbox (Dovecot passwd-file)"
	@echo "  make add-domain DOMAIN=... [SELECTOR]   Add new mail domain + DKIM"
	@echo "  make reload							 Reload services"
	@echo "  make restart							Restart services"
	@echo "  make logs							   Tail logs"

install: certs
	@mkdir -p data/{ssl,postfix,spool,opendkim/{conf,keys},dovecot-conf,dovecot,mail}
	@for f in opendkim.conf KeyTable SigningTable TrustedHosts; do \
		[ -f "data/opendkim/conf/$$f" ] || cp "opendkim/$$f" "data/opendkim/conf/$$f"; \
	done
	@docker-compose up -d
	@$(MAKE) reload
	@$(MAKE) test

test:
	@echo "Testing Submission (587 STARTTLS) and IMAPS (993)..."
	@SNI="$${MAIL_HOST:-localhost}"; echo "QUIT" | openssl s_client -connect 127.0.0.1:587 -starttls smtp -crlf -servername "$$SNI"
	@SNI="$${MAIL_HOST:-localhost}"; echo -e "a1 CAPABILITY\na2 LOGOUT" | openssl s_client -quiet -connect 127.0.0.1:993 -servername "$$SNI"

send:
	@[ -n "$(TO)" ] || (echo "Usage: make send TO=you@example.com SUBMISSION_USER=... SUBMISSION_PASS=..." && exit 1)
	@docker exec postfix bash -lc "swaks --server 127.0.0.1:587 \
	 --auth-user '$(SUBMISSION_USER)' \
	 --auth-password '$(SUBMISSION_PASS)' \
	 --tls --from '$(SUBMISSION_USER)' --to '$(TO)'"

certs:
	@[ -f data/ssl/cert.pem ] && echo "TLS cert exists" || $(MAKE) certs-force

certs-force:
	@mkdir -p data/ssl
	@CN="$${MAIL_HOST:-localhost}"; \
	openssl req -x509 -nodes -newkey rsa:2048 -sha256 \
	 -subj "/CN=$$CN" \
	 -addext "subjectAltName=DNS:$$CN" \
	 -keyout data/ssl/key.pem -out data/ssl/cert.pem -days 365
	@chmod 600 data/ssl/key.pem
	@chmod 644 data/ssl/cert.pem

add-user:
	@[ -n "$(USER)" ] && [ -n "$(PASS)" ] || (echo "Usage: make add-user USER=me@example.com PASS=secret" && exit 1)
	@docker exec dovecot bash -lc "HASH=\$$(doveadm pw -s SHA512-CRYPT -p '$(PASS)'); \
	  touch /etc/dovecot/passwd; \
	  grep -q '^$(USER):' /etc/dovecot/passwd && sed -i 's#^$(USER):.*#$(USER):'$${HASH}'#' /etc/dovecot/passwd || echo '$(USER):'$${HASH} >> /etc/dovecot/passwd; \
	  chown dovecot:dovecot /etc/dovecot/passwd; chmod 640 /etc/dovecot/passwd"

add-domain:
	@[ -n "$(DOMAIN)" ] || (echo "Usage: make add-domain DOMAIN=example.net [SELECTOR]" && exit 1)
	@docker exec opendkim bash -lc "/scripts/add-domain.sh $(DOMAIN) $${SELECTOR:-default}"
	@echo "Remember to add DNS records for $(DOMAIN)"

reload:
	@docker-compose exec postfix postfix reload
	@docker-compose exec dovecot dovecot reload || true
	@docker-compose exec opendkim pkill -HUP opendkim || true

restart:
	@docker-compose restart

logs:
	@docker-compose logs -f postfix opendkim dovecot