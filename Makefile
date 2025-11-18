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

.PHONY: help validate install test send certs certs-force add-user add-domain reload restart logs backup-dkim reports view-reports tail-reports build-rust test-pixel pixel-stats pixel-health pixel-logs pixel-debug verify-pixelmilter update-config fix-ownerships

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
	@echo ""
	@echo "Pixel Tracking Commands:"
	@echo "  make build-rust						Build Rust pixel tracking components"
	@echo "  make test-pixel						Test pixel tracking system"
	@echo "  make pixel-health					  Check pixel server health"
	@echo "  make pixel-stats					   View pixel tracking statistics"
	@echo "  make verify-pixelmilter				Verify pixelmilter is correctly configured"
	@echo ""
	@echo "Configuration Management:"
	@echo "  make update-config					Rebuild and reload services after config changes"
	@echo "  make reload							Reload services (use after editing .cf templates)"
	@echo "  make fix-ownerships					Fix ownerships of directories and files"

validate:
	@echo "Checking required dependencies..."
	@command -v openssl >/dev/null || (echo "ERROR: Missing openssl (required for TLS certificates)" && exit 1)
	@command -v docker >/dev/null || (echo "ERROR: Missing docker (required for containers)" && exit 1)
	@$(DOCKER_COMPOSE) version >/dev/null 2>&1 || (echo "ERROR: Missing docker-compose or 'docker compose' (required for orchestration)" && exit 1)
	@echo "✓ All required dependencies present"
	@echo "Optional tools:"
	@command -v swaks >/dev/null && echo "✓ swaks (for email testing)" || echo "⚠ swaks missing (install for email testing: apt install swaks)"
	@command -v jq >/dev/null && echo "✓ jq (for JSON parsing)" || echo "⚠ jq missing (install for better reports: apt install jq)"

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

# Rust Pixel Tracking Targets
build-rust:
	@echo "Building Rust pixel tracking components..."
	$(Q)$(DOCKER_COMPOSE) build pixelmilter pixelserver
	@echo "✓ Rust components built successfully"

test-pixel:
	@echo "Testing pixel tracking system..."
	@echo "1. Checking pixelserver health..."
	@curl -k -s "https://${MAIL_HOST:-localhost}:8443/health" | jq . 2>/dev/null || \
		curl -k -s "https://${MAIL_HOST:-localhost}:8443/health" || \
		echo "⚠ Health check failed - ensure pixelserver is running"
	@echo ""
	@echo "2. Testing pixel endpoint..."
	@curl -k -s -w "HTTP Status: %{http_code}\n" "https://${MAIL_HOST:-localhost}:8443/pixel?id=test-$(shell date +%s)" -o /dev/null || \
		echo "⚠ Pixel endpoint test failed"
	@echo ""
	@echo "3. Checking container status..."
	@$(DOCKER_COMPOSE) ps pixelmilter pixelserver

pixel-health:
	@echo "Pixel Server Health Check:"
	@curl -k -s "https://${MAIL_HOST:-localhost}:8443/health" | jq . 2>/dev/null || \
		curl -k -s "https://${MAIL_HOST:-localhost}:8443/health" || \
		echo "ERROR: Cannot reach pixel server health endpoint"

pixel-stats:
	@echo "Pixel Tracking Statistics:"
	@curl -k -s "https://${MAIL_HOST:-localhost}:8443/stats" | jq . 2>/dev/null || \
		curl -k -s "https://${MAIL_HOST:-localhost}:8443/stats" || \
		echo "ERROR: Cannot reach pixel server stats endpoint"

pixel-logs:
	@echo "Recent pixel tracking logs:"
	@$(DOCKER_COMPOSE) logs --tail=50 pixelmilter pixelserver

pixel-debug:
	@echo "Pixel Tracking Debug Information:"
	@echo "=== Container Status ==="
	@$(DOCKER_COMPOSE) ps pixelmilter pixelserver
	@echo ""
	@echo "=== Pixelmilter Logs (last 20 lines) ==="
	@$(DOCKER_COMPOSE) logs --tail=20 pixelmilter
	@echo ""
	@echo "=== Pixelserver Logs (last 20 lines) ==="
	@$(DOCKER_COMPOSE) logs --tail=20 pixelserver
	@echo ""
	@echo "=== Socket Status ==="
	@ls -la data/pixel/socket/ 2>/dev/null || echo "Socket directory not found"
	@echo ""
	@echo "=== Data Directory ==="
	@ls -la data/pixel/ 2>/dev/null | head -10 || echo "Data directory not found"

verify-pixelmilter:
	@echo "Verifying pixelmilter configuration..."
	@echo ""
	@echo "1. Checking pixelmilter container status..."
	@if $(DOCKER_COMPOSE) ps pixelmilter | grep -q "Up"; then \
		echo "✓ pixelmilter container is running"; \
	else \
		echo "✗ pixelmilter container is not running"; \
		exit 1; \
	fi
	@echo ""
	@echo "2. Checking pixelmilter TCP port..."
	@if $(DOCKER_COMPOSE) exec -T pixelmilter nc -z localhost 8892 2>/dev/null || \
		$(DOCKER_COMPOSE) exec -T pixelmilter timeout 1 bash -c 'echo > /dev/tcp/localhost/8892' 2>/dev/null; then \
		echo "✓ pixelmilter is listening on port 8892"; \
	else \
		echo "⚠ Cannot verify pixelmilter TCP port (may be normal if just started)"; \
	fi
	@echo ""
	@echo "3. Checking Postfix configuration for pixelmilter..."
	@if $(DOCKER_COMPOSE) exec -T postfix grep -q "8892\|pixelmilter" /etc/postfix/main.cf 2>/dev/null; then \
		echo "✓ pixelmilter found in Postfix main.cf"; \
		$(DOCKER_COMPOSE) exec -T postfix grep "smtpd_milters\|non_smtpd_milters" /etc/postfix/main.cf | grep -v "^#" || true; \
	else \
		echo "✗ pixelmilter not found in Postfix main.cf"; \
		echo "  Run 'make update-config' to regenerate configuration"; \
		exit 1; \
	fi
	@echo ""
	@echo "4. Checking Postfix can connect to pixelmilter..."
	@if $(DOCKER_COMPOSE) exec -T postfix nc -z $${PIXEL_MILTER_IP:-172.18.0.5} 8892 2>/dev/null || \
		$(DOCKER_COMPOSE) exec -T postfix timeout 1 bash -c "echo > /dev/tcp/$${PIXEL_MILTER_IP:-172.18.0.5}/8892" 2>/dev/null; then \
		echo "✓ Postfix can connect to pixelmilter on TCP port 8892"; \
	else \
		echo "⚠ Postfix cannot connect to pixelmilter (check network connectivity)"; \
	fi
	@echo ""
	@echo "5. Checking pixelmilter process..."
	@if $(DOCKER_COMPOSE) exec -T pixelmilter pgrep -f pixelmilter >/dev/null 2>&1; then \
		echo "✓ pixelmilter process is running"; \
	else \
		echo "✗ pixelmilter process not found"; \
		exit 1; \
	fi
	@echo ""
	@echo "6. Checking Postfix milter status..."
	@if $(DOCKER_COMPOSE) exec -T postfix postconf smtpd_milters 2>/dev/null | grep -q "8892\|pixelmilter"; then \
		echo "✓ Postfix smtpd_milters includes pixelmilter (port 8892)"; \
		$(DOCKER_COMPOSE) exec -T postfix postconf smtpd_milters | head -1; \
	else \
		echo "✗ Postfix smtpd_milters does not include pixelmilter"; \
		echo "  Run 'make update-config' to regenerate configuration"; \
		exit 1; \
	fi
	@echo ""
	@echo "✓ All pixelmilter verification checks passed!"

update-config:
	@echo "Updating configuration files..."
	@echo ""
	@echo "1. Rebuilding Postfix container to apply template changes..."
	$(Q)$(DOCKER_COMPOSE) build postfix
	@echo ""
	@echo "2. Restarting Postfix to load new configuration..."
	$(Q)$(DOCKER_COMPOSE) restart postfix
	@echo ""
	@echo "3. Waiting for Postfix to be ready..."
	@sleep 3
	@echo ""
	@echo "4. Verifying Postfix configuration..."
	@if $(DOCKER_COMPOSE) exec -T postfix postfix check >/dev/null 2>&1; then \
		echo "✓ Postfix configuration is valid"; \
	else \
		echo "✗ Postfix configuration check failed"; \
		$(DOCKER_COMPOSE) exec -T postfix postfix check || true; \
		exit 1; \
	fi
	@echo ""
	@echo "5. Reloading Postfix to apply changes..."
	$(Q)$(DOCKER_COMPOSE) exec postfix postfix reload
	@echo ""
	@echo "✓ Configuration updated successfully!"
	@echo ""
	@echo "Note: If you modified pixelmilter configuration, you may also need to:"
	@echo "  - Restart pixelmilter: make restart (or docker-compose restart pixelmilter)"
	@echo "  - Verify configuration: make verify-pixelmilter"

fix-ownerships:
	@echo "Fixing ownerships of directories and files..."
	@echo ""
	@echo "This command requires sudo privileges to change ownership."
	@echo ""
	@# Get UIDs/GIDs from running containers or use defaults
	@DOVECOT_UID=$$(docker compose exec -T dovecot id -u dovecot 2>/dev/null || echo "100"); \
	DOVECOT_GID=$$(docker compose exec -T dovecot id -g dovecot 2>/dev/null || echo "102"); \
	POSTFIX_UID=$$(docker compose exec -T postfix id -u postfix 2>/dev/null || echo "100"); \
	POSTFIX_GID=$$(docker compose exec -T postfix id -g postfix 2>/dev/null || echo "102"); \
	PIXEL_UID=$$(docker compose exec -T pixelmilter id -u pixel 2>/dev/null || echo "999"); \
	PIXEL_GID=$$(docker compose exec -T pixelmilter id -g pixel 2>/dev/null || echo "999"); \
	PIXELSERVER_UID=$$(docker compose exec -T pixelserver id -u pixelserver 2>/dev/null || echo "999"); \
	PIXELSERVER_GID=$$(docker compose exec -T pixelserver id -g pixelserver 2>/dev/null || echo "999"); \
	echo "Detected UIDs/GIDs:"; \
	echo "  dovecot:   uid=$$DOVECOT_UID gid=$$DOVECOT_GID"; \
	echo "  postfix:   uid=$$POSTFIX_UID gid=$$POSTFIX_GID"; \
	echo "  pixel:     uid=$$PIXEL_UID gid=$$PIXEL_GID"; \
	echo "  pixelserver: uid=$$PIXELSERVER_UID gid=$$PIXELSERVER_GID"; \
	echo ""; \
	echo "Fixing data/mail ownership (dovecot)..."; \
	sudo chown -R $$DOVECOT_UID:$$DOVECOT_GID data/mail 2>/dev/null || echo "  ⚠ Could not change ownership of data/mail (may need manual fix)"; \
	echo "Fixing data/pixel ownership (pixelmilter/pixelserver)..."; \
	sudo chown -R $$PIXEL_UID:$$PIXEL_GID data/pixel 2>/dev/null || echo "  ⚠ Could not change ownership of data/pixel (may need manual fix)"; \
	echo "Fixing data/logs ownership..."; \
	sudo chown -R root:root data/logs 2>/dev/null || echo "  ⚠ Could not change ownership of data/logs (may need manual fix)"; \
	sudo chmod 755 data/logs 2>/dev/null || true; \
	if [ -f data/logs/dovecot.log ]; then \
		sudo chown $$DOVECOT_UID:$$DOVECOT_GID data/logs/dovecot.log 2>/dev/null || true; \
		sudo chmod 644 data/logs/dovecot.log 2>/dev/null || true; \
	fi; \
	if [ -f data/logs/postfix.log ]; then \
		sudo chown $$POSTFIX_UID:$$POSTFIX_GID data/logs/postfix.log 2>/dev/null || true; \
		sudo chmod 644 data/logs/postfix.log 2>/dev/null || true; \
	fi; \
	echo "Fixing SSL certificates ownership..."; \
	# Ensure host-side SSL files are owned by root so bind-mounts present root-owned files inside containers
	sudo chown -R root:root ssl 2>/dev/null || echo "  ⚠ Could not change ownership of ssl (may need manual fix)"; \
	sudo chmod 755 ssl 2>/dev/null || true; \
	if [ -f ssl/cert.pem ]; then \
		sudo chown root:root ssl/cert.pem 2>/dev/null || true; \
		sudo chmod 644 ssl/cert.pem 2>/dev/null || true; \
	fi; \
	if [ -f ssl/key.pem ]; then \
		sudo chown root:root ssl/key.pem 2>/dev/null || true; \
		sudo chmod 600 ssl/key.pem 2>/dev/null || true; \
	fi; \
	if [ -d ssl/opendkim ]; then \
		sudo chown -R root:root ssl/opendkim 2>/dev/null || true; \
		sudo chmod 755 ssl/opendkim 2>/dev/null || true; \
		if [ -d ssl/opendkim/keys ]; then \
			sudo chmod -R 755 ssl/opendkim/keys 2>/dev/null || true; \
		fi; \
	fi; \
	# Also ensure any generated certs under data/ssl are owned correctly
	if [ -d data/ssl ]; then \
		sudo chown -R root:root data/ssl 2>/dev/null || true; \
		sudo chmod -R 755 data/ssl 2>/dev/null || true; \
		if [ -f data/ssl/key.pem ]; then \
			sudo chown root:root data/ssl/key.pem 2>/dev/null || true; \
			sudo chmod 600 data/ssl/key.pem 2>/dev/null || true; \
		fi; \
	fi; \
	echo "Fixing dovecot/passwd ownership..."; \
	if [ -f dovecot/passwd ]; then \
		sudo chown $$DOVECOT_UID:$$DOVECOT_GID dovecot/passwd 2>/dev/null || echo "  ⚠ Could not change ownership of dovecot/passwd (may need manual fix)"; \
		sudo chmod 640 dovecot/passwd 2>/dev/null || true; \
	fi; \
	echo "Fixing postfix/resolv.conf ownership..."; \
	if [ -f postfix/resolv.conf ]; then \
		sudo chown root:root postfix/resolv.conf 2>/dev/null || echo "  ⚠ Could not change ownership of postfix/resolv.conf (may need manual fix)"; \
		sudo chmod 644 postfix/resolv.conf 2>/dev/null || true; \
	fi; \
	echo ""; \
	echo "✓ Ownership fixes completed!"
