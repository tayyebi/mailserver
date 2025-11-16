SHELL := /bin/bash
.DEFAULT_GOAL := help

# Support both docker-compose and the new docker compose CLI
DOCKER_COMPOSE := $(shell command -v docker-compose 2>/dev/null || echo docker compose)

# Verbose toggle: set VERBOSE=1 to echo every command
VERBOSE ?= 0
Q := $(if $(filter 1,$(VERBOSE)),,@)

ifneq (,$(wildcard ./.env))
include .env
export
endif

.PHONY: help validate install test send certs certs-force add-user add-domain reload restart logs backup-dkim reports view-reports tail-reports

help:
	@echo "Available targets:"
	@echo "  make validate						  Check for required binaries"
	@echo "  make install						   Bootstrap all services and data"
	@echo "  make test							  Run mailserver health checks"
	@echo "  make send TO=addr SUBMISSION_USER=..   Send test email over submission"
	@echo "  make certs							 Generate TLS certs if missing"
	@echo "  make certs-force					   Regenerate TLS certs"
	@echo "  make add-user USER=.. PASS=..		  Add/update a mailbox"
	@echo "  make add-domain DOMAIN=.. [SELECTOR]   Add new mail domain + DKIM"
	@echo "  make reload							Reload services"
	@echo "  make restart						   Restart services"
	@echo "  make logs							  Tail logs"
	@echo "  make backup-dkim					   Backup DKIM keys"

validate:
	@command -v openssl >/dev/null || (echo "Missing: openssl" && exit 1)
	@command -v swaks   >/dev/null || (echo "Missing: swaks"   && exit 1)
	@echo "All required binaries present."

install: validate certs
	$(Q)mkdir -p data/{ssl,postfix,spool,opendkim/{conf,keys},dovecot-conf,dovecot,mail,pixel/socket}
	$(Q)$(DOCKER_COMPOSE) up -d

test:
	@echo "Testing Submission (587 STARTTLS) and IMAPS (993)..."
	@SNI="$${MAIL_HOST:-localhost}"; echo "QUIT" | openssl s_client \
	   -connect 127.0.0.1:587 -starttls smtp -crlf -servername "$$SNI"
	@SNI="$${MAIL_HOST:-localhost}"; echo -e "a1 CAPABILITY\na2 LOGOUT" | openssl s_client \
	   -quiet -connect 127.0.0.1:993 -servername "$$SNI"

certs:
	@[ -f data/ssl/cert.pem ] && echo "TLS cert exists" || $(MAKE) certs-force

certs-force:
	@mkdir -p data/ssl
	@CN="$${MAIL_HOST:-localhost}"; \
	 echo "[$$(date -u)] Generating TLS cert for CN=$$CN"; \
	 openssl req -x509 -nodes -newkey rsa:2048 -sha256 \
		   -subj "/CN=$$CN" \
		   -addext "subjectAltName=DNS:$$CN" \
		   -keyout data/ssl/key.pem -out data/ssl/cert.pem -days 365
	@chmod 600 data/ssl/key.pem
	@chmod 644 data/ssl/cert.pem

add-user:
	@[ -n "$(USER)" ] && [ -n "$(PASS)" ] || (echo "Usage: make add-user USER=me@example.com PASS=secret" && exit 1)
	$(Q)HASH=$$(docker run --rm dovecot bash -lc "doveadm pw -s SHA512-CRYPT -p '$(PASS)'"); \
	docker exec dovecot bash -lc "\
	  touch /etc/dovecot/passwd; \
	  grep -q '^$(USER):' /etc/dovecot/passwd \
		&& sed -i 's#^$(USER):.*#$(USER):'$${HASH}'#' /etc/dovecot/passwd \
		|| echo '$(USER):'$${HASH} >> /etc/dovecot/passwd; \
	  chown dovecot:dovecot /etc/dovecot/passwd; \
	  chmod 640 /etc/dovecot/passwd"

show-users:
	$(Q)docker exec dovecot bash -lc "cat /etc/dovecot/passwd"

remove-user:
	@[ -n "$(USER)" ] || (echo "Usage: make remove-user USER=me@example.com" && exit 1)
	$(Q)docker exec dovecot bash -lc "\
	  sed -i '/^$(USER):/d' /etc/dovecot/passwd"

add-domain:
	@[ -n "$(DOMAIN)" ] || (echo "Usage: make add-domain DOMAIN=example.net [SELECTOR]" && exit 1)
	$(Q)docker exec opendkim bash -lc "/scripts/add-domain.sh $(DOMAIN) $${SELECTOR:-default}"
	@echo "Remember to add DNS records for $(DOMAIN)"

reload:
	$(Q)$(DOCKER_COMPOSE) exec postfix postfix reload
	$(Q)$(DOCKER_COMPOSE) exec dovecot dovecot reload || true
	$(Q)$(DOCKER_COMPOSE) exec opendkim pkill -HUP opendkim || true

restart:
	$(Q)$(DOCKER_COMPOSE) restart

logs:
	$(Q)$(DOCKER_COMPOSE) logs -f postfix opendkim dovecot

backup-dkim:
	@tar czf dkim-backup-$$\(date +%Y%m%d_%H%M%S\).tgz -C data/opendkim keys
	@echo "DKIM keys backed up to $$(ls -1 dkim-backup-*.tgz | tail -n1)"

render-maps:
	$(Q)$(DOCKER_COMPOSE) exec postfix bash -c "\
		postmap /etc/postfix/virtual_aliases && \
		postmap /etc/postfix/virtual_domains && \
		postmap /etc/postfix/vmailbox"

reports:
    @PAGE=$${PAGE:-1}; PER=$${PER:-50}; \
    URL="https://${MAIL_HOST:-localhost}:8443/reports?page=$$PAGE&per=$$PER&format=html"; \
    echo "Fetching $$URL"; \
    curl -k --silent "$$URL" | sed -n '1,300p'

view-reports:
    @PAGE=$${PAGE:-1}; PER=$${PER:-50}; \
    URL="https://${MAIL_HOST:-localhost}:8443/reports?page=$$PAGE&per=$$PER"; \
    echo "GET $$URL"; \
    curl -k --silent "$$URL" | jq .

tail-reports:
    @LOG=data/pixel/requests.log; \
    if [ -f "$$LOG" ]; then tail -n 200 "$$LOG" || true; else echo "No log file at $$LOG"; fi
