SHELL := /bin/bash
.DEFAULT_GOAL := help

.PHONY: help install test send certs certs-force add-user add-domain reload restart logs

help:
	@echo "Available targets:"
	@echo "  make install              Bootstrap all services and data"
	@echo "  make test                 Run mailserver health checks"
	@echo "  make send TO=addr         Send test email over submission"
	@echo "  make certs                Generate TLS certs if missing"
	@echo "  make certs-force          Regenerate TLS certs"
	@echo "  make add-user USER=... PASS=...    Add/update a mailbox"
	@echo "  make add-domain DOMAIN=...         Add new mail domain + DKIM"
	@echo "  make reload               Reload services"
	@echo "  make restart              Restart services"
	@echo "  make logs                 Tail logs"

install: certs
	@mkdir -p data/{ssl,postfix,spool,opendkim/{conf,keys},dovecot-conf,dovecot,mail}
	@docker-compose up -d
	@$(MAKE) reload
	@$(MAKE) test

test:
	@echo "Testing Postfix submission (STARTTLS) and Dovecot IMAPS..."
	@echo "QUIT" | openssl s_client -connect 127.0.0.1:587 -starttls smtp -crlf
	@echo "QUIT" | openssl s_client -connect 127.0.0.1:993

send:
	@[ -n "$(TO)" ] || (echo "Usage: make send TO=you@example.com SUBMISSION_USER=... SUBMISSION_PASS=..." && exit 1)
	@docker exec postfix bash -c "swaks --server 127.0.0.1:587 \
	--auth-user \"$(SUBMISSION_USER)\" \
	--auth-password \"$(SUBMISSION_PASS)\" \
	--tls --from \"$(SUBMISSION_USER)\" --to \"$(TO)\""

certs:
	@[ -f data/ssl/cert.pem ] && echo "TLS cert exists" || $(MAKE) certs-force

certs-force:
	@mkdir -p data/ssl
	@openssl req -x509 -nodes -newkey rsa:2048 \
	-subj "/CN=$(MAIL_HOST)" \
	-keyout data/ssl/key.pem -out data/ssl/cert.pem -days 365

add-user:
	@[ -n "$(USER)" ] && [ -n "$(PASS)" ] || (echo "Usage: make add-user USER=me@example.com PASS=secret" && exit 1)
	@docker exec dovecot doveadm user add "$(USER)" "$(PASS)"

add-domain:
	@[ -n "$(DOMAIN)" ] || (echo "Usage: make add-domain DOMAIN=example.net" && exit 1)
	@docker exec opendkim bash -c "/scripts/add-domain.sh $(DOMAIN)"
	@echo "Remember to add DNS records for $(DOMAIN)"

reload:
	@docker-compose exec postfix postfix reload
	@docker-compose exec opendkim pkill -HUP opendkim || true
	@docker-compose exec dovecot dovecot reload

restart:
	@docker-compose restart

logs:
	@docker-compose logs -f postfix opendkim dovecot