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

.PHONY: help validate install test test-connectivity send certs certs-force add-user add-domain show-dkim reload restart logs backup-dkim reports view-reports tail-reports build-rust test-pixel pixel-stats pixel-health pixel-logs pixel-debug verify-pixelmilter update-config fix-ownerships queue-status queue-flush outbound-status

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
	@echo "  make show-dkim DOMAIN=.. [SELECTOR]   Show DKIM DNS record for domain"
	@echo "  make reload							Reload services"
	@echo "  make restart						   Restart services"
	@echo "  make logs							  Tail logs"
	@echo "  make backup-dkim					   Backup DKIM keys"
	@echo "  make queue-status					  Display emails in queue"
	@echo "  make queue-flush					   Flush email queue"
	@echo "  make outbound-status				   Display last outbound emails status"
	@echo "  make send TO=addr [FROM=addr] [SUBJECT=...]  Send test email (reads password from passwd or .env)"
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
	@echo ""
	@echo "Git Commands:"
	@echo "  make pull							Force pull from remote (stashes local changes)"

validate:
	@echo "Checking required dependencies..."
	@command -v openssl >/dev/null || (echo "ERROR: Missing openssl (required for TLS certificates)" && exit 1)
	@command -v docker >/dev/null || (echo "ERROR: Missing docker (required for containers)" && exit 1)
	@$(DOCKER_COMPOSE) version >/dev/null 2>&1 || (echo "ERROR: Missing docker-compose or 'docker compose' (required for orchestration)" && exit 1)
	@echo "✓ All required dependencies present"
	@echo ""
	@echo "Checking permissions..."
	@if [ -w /root ] 2>/dev/null || [ "$$(id -u)" = "0" ]; then \
		echo "✓ Running as root - sudo not needed"; \
	elif command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then \
		echo "✓ Sudo available and passwordless access configured"; \
	elif command -v sudo >/dev/null 2>&1; then \
		echo "⚠ Sudo available but may require password (some commands may prompt)"; \
	else \
		echo "⚠ Sudo not available - some commands may fail if root privileges are needed"; \
	fi
	@echo ""
	@echo "Optional tools:"
	@command -v swaks >/dev/null && echo "✓ swaks (for email testing)" || echo "⚠ swaks missing (install for email testing: apt install swaks)"
	@command -v jq >/dev/null && echo "✓ jq (for JSON parsing)" || echo "⚠ jq missing (install for better reports: apt install jq)"

install: validate certs
	$(Q)mkdir -p data/{ssl,postfix,spool,opendkim/{conf,keys},dovecot-conf,dovecot,mail,pixel/socket}
	$(Q)touch data/{logs/dovecot.log,logs/postfix.log}
	$(Q)cp .env.example .env
	@echo "Please update .env"
	$(Q)cp dovecot/passwd.example data/dovecot/passwd
	@echo "Please update data/dovecot/passwd"


test:
	@echo "\n==[ Mailserver Health Checks ]=="
	@echo "1. TLS/IMAPS connectivity (993):"
	@SNI="$${MAIL_HOST:-localhost}"; echo -e "a1 CAPABILITY\na2 LOGOUT" | openssl s_client -quiet -connect 127.0.0.1:993 -servername "$$SNI" 2>&1 | grep -q 'OK' \
		&& echo "✓ IMAPS TLS works" \
		|| (echo "✗ IMAPS TLS failed" && exit 1)
	@echo "2. Submission (587 STARTTLS):"
	@SNI="$${MAIL_HOST:-localhost}"; echo "QUIT" | openssl s_client -connect 127.0.0.1:587 -starttls smtp -crlf -servername "$$SNI" 2>&1 | grep -q '250' \
		&& echo "✓ Submission STARTTLS works" \
		|| (echo "✗ Submission STARTTLS failed" && exit 1)
	@echo "3. Dovecot liveness:"
	@$(DOCKER_COMPOSE) exec -T dovecot doveadm who 2>/dev/null && echo "✓ Dovecot responds" || (echo "✗ Dovecot not responding" && exit 1)
	@echo "4. Postfix liveness:"
	@$(DOCKER_COMPOSE) exec -T postfix postfix status 2>/dev/null && echo "✓ Postfix responds" || (echo "✗ Postfix not responding" && exit 1)
	@echo "5. SASL auth service (Dovecot port 12345):"
	@if $(DOCKER_COMPOSE) exec -T dovecot ss -tlnp 2>/dev/null | grep -q ":12345 " || \
		$(DOCKER_COMPOSE) exec -T dovecot netstat -tlnp 2>/dev/null | grep -q ":12345 "; then \
		echo "✓ Dovecot SASL service listening on port 12345"; \
	else \
		echo "✗ Dovecot SASL service NOT listening on port 12345"; \
		echo "  Checking configuration..."; \
		if grep -q "^ service auth {" dovecot/dovecot.conf 2>/dev/null; then \
			echo "  ⚠ Found leading space in 'service auth' - fixing..."; \
			sed -i 's/^ service auth {/service auth {/' dovecot/dovecot.conf; \
			echo "  ✓ Fixed. Restart Dovecot: make restart"; \
		fi; \
		if [ ! -f data/dovecot/passwd ] || [ -d data/dovecot/passwd ]; then \
			echo "  ⚠ data/dovecot/passwd is missing or a directory - creating file..."; \
			rm -rf data/dovecot/passwd; touch data/dovecot/passwd; chmod 644 data/dovecot/passwd; \
			echo "  ✓ Created. Add users with: make add-user USER=... PASS=..."; \
		fi; \
		exit 1; \
	fi
	@echo "6. SASL connectivity (Postfix→Dovecot):"
	@if $(DOCKER_COMPOSE) exec -T postfix timeout 2 bash -c "echo > /dev/tcp/$${DOVECOT_IP:-172.18.0.5}/12345" 2>/dev/null; then \
		echo "✓ Postfix can connect to Dovecot SASL"; \
	else \
		echo "✗ Postfix cannot connect to Dovecot SASL (check DOVECOT_IP in .env)"; \
		exit 1; \
	fi
	@echo "7. pixelmilter health:"
	@$(MAKE) verify-pixelmilter
	@echo "8. Connectivity and Network Checks:"
	@$(MAKE) test-connectivity
	@echo "9. Log error scan (last 50 lines):"
	@tail -n 50 data/logs/dovecot.log 2>/dev/null | grep -iE 'error|fail|fatal' && echo "⚠ Dovecot log errors found" || echo "✓ Dovecot logs clean"
	@tail -n 50 data/logs/postfix.log 2>/dev/null | grep -iE 'error|fail|fatal' && echo "⚠ Postfix log errors found" || echo "✓ Postfix logs clean"
	@echo "\n==[ All health checks complete ]=="

test-connectivity:
	@echo ""
	@echo "  a. Checking DNS resolution..."
	@MAIL_HOST="$${MAIL_HOST:-localhost}"; \
	if [ "$$MAIL_HOST" != "localhost" ] && [ "$$MAIL_HOST" != "127.0.0.1" ]; then \
		echo "    Checking DNS for $$MAIL_HOST..."; \
		if host "$$MAIL_HOST" >/dev/null 2>&1 || getent hosts "$$MAIL_HOST" >/dev/null 2>&1; then \
			echo "    ✓ DNS resolves for $$MAIL_HOST"; \
			host "$$MAIL_HOST" 2>/dev/null | head -3 || getent hosts "$$MAIL_HOST" 2>/dev/null | head -1; \
		else \
			echo "    ✗ DNS resolution failed for $$MAIL_HOST"; \
			echo "    ⚠  This may cause external clients to fail connecting"; \
		fi; \
	else \
		echo "    ⚠  MAIL_HOST is localhost - external clients cannot connect"; \
	fi
	@echo ""
	@echo "  b. Checking port binding on host (0.0.0.0 vs 127.0.0.1)..."
	@for port in 25 587 465 993 143; do \
		if command -v ss >/dev/null 2>&1; then \
			BINDING=$$(ss -tlnp 2>/dev/null | grep ":$$port " | head -1); \
		elif command -v netstat >/dev/null 2>&1; then \
			BINDING=$$(netstat -tlnp 2>/dev/null | grep ":$$port " | head -1); \
		else \
			BINDING=""; \
		fi; \
		if [ -n "$$BINDING" ]; then \
			if echo "$$BINDING" | grep -q "0.0.0.0:$$port\|:::$$port"; then \
				echo "    ✓ Port $$port is bound to 0.0.0.0 (accessible externally)"; \
			elif echo "$$BINDING" | grep -q "127.0.0.1:$$port"; then \
				echo "    ✗ Port $$port is only bound to 127.0.0.1 (NOT accessible externally)"; \
			else \
				echo "    ⚠  Port $$port binding: $$BINDING"; \
			fi; \
		else \
			echo "    ✗ Port $$port is not listening on host"; \
		fi; \
	done
	@echo ""
	@echo "  c. Testing SMTP port connectivity with timeout..."
	@MAIL_HOST="$${MAIL_HOST:-localhost}"; \
	for port in 25 587 465; do \
		echo -n "    Testing port $$port... "; \
		if timeout 3 bash -c "echo > /dev/tcp/127.0.0.1/$$port" 2>/dev/null; then \
			echo "✓ Port $$port is reachable on localhost"; \
		else \
			echo "✗ Port $$port is NOT reachable on localhost"; \
		fi; \
	done
	@echo ""
	@echo "  d. Testing SMTP service response (with timeout)..."
	@MAIL_HOST="$${MAIL_HOST:-localhost}"; \
	for port in 25 587; do \
		echo -n "    SMTP port $$port response... "; \
		RESPONSE=$$(timeout 3 bash -c 'exec 3<>/dev/tcp/127.0.0.1/'"$$port"'; echo "QUIT" >&3; cat <&3' 2>/dev/null | head -1 || echo ""); \
		if echo "$$RESPONSE" | grep -qiE "220|250|354"; then \
			echo "✓ Service responding ($$(echo $$RESPONSE | tr -d '\r\n' | cut -c1-50))"; \
		else \
			if timeout 1 bash -c "echo > /dev/tcp/127.0.0.1/$$port" 2>/dev/null; then \
				echo "⚠ Port open but no SMTP banner (may be starting up)"; \
			else \
				echo "✗ No valid SMTP response (timeout or connection refused)"; \
			fi; \
		fi; \
	done
	@echo ""
	@echo "  e. Checking Docker port mappings..."
	@if $(DOCKER_COMPOSE) ps postfix 2>/dev/null | grep -q "Up"; then \
		echo "    Postfix container ports:"; \
		docker port postfix 2>/dev/null | grep -E "25|587|465" || echo "      ⚠  Could not retrieve port mappings"; \
	else \
		echo "    ✗ Postfix container is not running"; \
	fi
	@if $(DOCKER_COMPOSE) ps dovecot 2>/dev/null | grep -q "Up"; then \
		echo "    Dovecot container ports:"; \
		docker port dovecot 2>/dev/null | grep -E "993|143|110|995" || echo "      ⚠  Could not retrieve port mappings"; \
	else \
		echo "    ✗ Dovecot container is not running"; \
	fi
	@echo ""
	@echo "  f. Checking firewall rules (if accessible)..."
	@HAS_BLOCKS=0; \
	UFW_CHECKED=0; \
	if command -v ufw >/dev/null 2>&1; then \
		UFW_STATUS=$$(ufw status 2>/dev/null | head -1 || echo ""); \
		UFW_CHECKED=1; \
		if echo "$$UFW_STATUS" | grep -qi "active"; then \
			echo "    UFW firewall is active:"; \
			for port in 25 587 465 993 143; do \
				UFW_RULE=$$(ufw status 2>/dev/null | grep -E "$$port" || true); \
				if [ -n "$$UFW_RULE" ]; then \
					echo "      Port $$port: $$UFW_RULE"; \
				else \
					echo "      ⚠  Port $$port: No explicit rule (may be blocked by default)"; \
					HAS_BLOCKS=1; \
				fi; \
			done; \
		else \
			echo "    ✓ UFW firewall is inactive"; \
		fi; \
	fi; \
	if command -v iptables >/dev/null 2>&1 && ([ -w /proc/sys/net/ipv4/ip_forward ] 2>/dev/null || [ "$$(id -u)" = "0" ]); then \
		for port in 25 587 465 993 143; do \
			RULES=$$(iptables -L INPUT -n --line-numbers 2>/dev/null | grep -E "DROP|REJECT.*:$$port" || true); \
			if [ -n "$$RULES" ]; then \
				echo "    ⚠  Port $$port may be blocked by iptables:"; \
				echo "$$RULES" | sed 's/^/      /'; \
				HAS_BLOCKS=1; \
			fi; \
		done; \
		if [ $$HAS_BLOCKS -eq 0 ] && [ $$UFW_CHECKED -eq 0 ]; then \
			echo "    ✓ No obvious firewall blocks detected (requires root to check fully)"; \
		fi; \
	elif [ $$UFW_CHECKED -eq 0 ]; then \
		echo "    ⚠  Cannot check firewall rules (requires root or iptables access)"; \
	fi
	@echo ""
	@echo "  g. Testing external connectivity simulation..."
	@MAIL_HOST="$${MAIL_HOST:-localhost}"; \
	if [ "$$MAIL_HOST" != "localhost" ] && [ "$$MAIL_HOST" != "127.0.0.1" ]; then \
		echo "    Testing connection to $$MAIL_HOST:587 (submission)..."; \
		if timeout 5 bash -c "echo > /dev/tcp/$$MAIL_HOST/587" 2>/dev/null; then \
			echo "    ✓ Can connect to $$MAIL_HOST:587 via hostname"; \
		else \
			echo "    ✗ Cannot connect to $$MAIL_HOST:587 via hostname"; \
			echo "    ⚠  External clients will likely timeout"; \
		fi; \
	else \
		echo "    ⚠  MAIL_HOST is localhost - skipping external connectivity test"; \
	fi
	@echo ""
	@echo "  h. Checking Postfix network configuration..."
	@if $(DOCKER_COMPOSE) exec -T postfix postconf inet_interfaces 2>/dev/null | grep -q "all\|0.0.0.0"; then \
		echo "    ✓ Postfix is configured to listen on all interfaces"; \
		$(DOCKER_COMPOSE) exec -T postfix postconf inet_interfaces 2>/dev/null | head -1; \
	else \
		echo "    ⚠  Postfix inet_interfaces:"; \
		$(DOCKER_COMPOSE) exec -T postfix postconf inet_interfaces 2>/dev/null | head -1; \
	fi
	@if $(DOCKER_COMPOSE) exec -T postfix postconf myhostname 2>/dev/null; then \
		echo "    Postfix myhostname:"; \
		$(DOCKER_COMPOSE) exec -T postfix postconf myhostname 2>/dev/null | head -1; \
	fi
	@echo ""
	@echo "  i. Summary and recommendations:"
	@MAIL_HOST="$${MAIL_HOST:-localhost}"; \
	ISSUES=0; \
	if [ "$$MAIL_HOST" = "localhost" ] || [ "$$MAIL_HOST" = "127.0.0.1" ]; then \
		echo "    ✗ MAIL_HOST is set to localhost - external clients cannot connect"; \
		ISSUES=$$((ISSUES + 1)); \
	fi; \
	if ! timeout 3 bash -c "echo > /dev/tcp/127.0.0.1/587" 2>/dev/null; then \
		echo "    ✗ Port 587 is not accessible - check Docker port mapping"; \
		ISSUES=$$((ISSUES + 1)); \
	fi; \
	if [ $$ISSUES -eq 0 ]; then \
		echo "    ✓ Basic connectivity checks passed"; \
		echo "    ℹ  If clients still timeout, check:"; \
		echo "       - Firewall rules (ufw/iptables/firewalld)"; \
		echo "       - Cloud provider security groups"; \
		echo "       - Router/NAT configuration"; \
		echo "       - DNS A/AAAA records point to server IP"; \
	fi

certs:
	@[ -f data/ssl/cert.pem ] && echo "TLS cert exists" || $(MAKE) certs-force

certs-force:
	@mkdir -p data/ssl
	@SUDO_CMD=$$(if [ "$$(id -u)" = "0" ] 2>/dev/null; then echo ""; elif command -v sudo >/dev/null 2>&1; then echo "sudo"; else echo ""; fi); \
	# Prevent ssl/key.pem and ssl/cert.pem from being directories; \
	[ -d ssl/key.pem ] && $$SUDO_CMD rm -rf ssl/key.pem || true; \
	[ -d ssl/cert.pem ] && $$SUDO_CMD rm -rf ssl/cert.pem || true; \
	[ -d data/ssl/key.pem ] && $$SUDO_CMD rm -rf data/ssl/key.pem || true; \
	[ -d data/ssl/cert.pem ] && $$SUDO_CMD rm -rf data/ssl/cert.pem || true; \
	CN="$${MAIL_HOST:-localhost}"; \
	echo "[$$(date -u)] Generating TLS cert for CN=$$CN"; \
	openssl req -x509 -nodes -newkey rsa:2048 -sha256 \
		   -subj "/CN=$$CN" \
		   -addext "subjectAltName=DNS:$$CN" \
		   -keyout data/ssl/key.pem -out data/ssl/cert.pem -days 365; \
	$$SUDO_CMD chmod 600 data/ssl/key.pem 2>/dev/null || chmod 600 data/ssl/key.pem; \
	$$SUDO_CMD chmod 644 data/ssl/cert.pem 2>/dev/null || chmod 644 data/ssl/cert.pem


# Add or update a user's password in data/dovecot/passwd
add-user update-user:
		@[ -n "$(USER)" ] && [ -n "$(PASS)" ] || (echo "Usage: make $@ USER=me@example.com PASS=secret" && exit 1)
		@# Ensure passwd file exists and is a file, not directory
		@if [ ! -f data/dovecot/passwd ]; then \
			if [ -d data/dovecot/passwd ]; then \
				echo "Removing data/dovecot/passwd directory..."; \
				rm -rf data/dovecot/passwd; \
			fi; \
			touch data/dovecot/passwd; \
			chmod 644 data/dovecot/passwd; \
		fi
		$(Q)HASH=$$($(DOCKER_COMPOSE) exec -T dovecot doveadm pw -s SHA512-CRYPT -p '$(PASS)' 2>&1 | grep -v "the input device" | tail -1); \
		if [ -z "$$HASH" ] || echo "$$HASH" | grep -q "TTY"; then \
			HASH=$$(docker run --rm $$($(DOCKER_COMPOSE) config | grep 'dovecot:' -A 5 | grep 'image:' | awk '{print $$2}' || echo 'dovecot:latest') doveadm pw -s SHA512-CRYPT -p '$(PASS)' 2>&1 | tail -1); \
		fi; \
		if [ -z "$$HASH" ]; then \
			echo "Error: Failed to generate password hash"; \
			exit 1; \
		fi; \
		if grep -q '^$(USER):' data/dovecot/passwd; then \
			sed -i "s#^$(USER):.*#$(USER):$${HASH}#" data/dovecot/passwd; \
			echo "Password updated for $(USER)"; \
		else \
			echo "$(USER):$${HASH}" >> data/dovecot/passwd; \
			echo "User added: $(USER)"; \
		fi; \
		chmod 644 data/dovecot/passwd

show-users:
	@echo "Users and password hashes (from data/dovecot/passwd):"
	@$(Q)docker exec dovecot bash -lc "cat /etc/dovecot/passwd || echo 'No passwd file found'"

remove-user:
	@[ -n "$(USER)" ] || (echo "Usage: make remove-user USER=me@example.com" && exit 1)
	$(Q)docker exec dovecot bash -lc "\
	  sed -i '/^$(USER):/d' /etc/dovecot/passwd"

add-domain:
	@[ -n "$(DOMAIN)" ] || (echo "Usage: make add-domain DOMAIN=example.net [SELECTOR]" && exit 1)
	@SELECTOR="$${SELECTOR:-default}"; \
	DOMAIN="$(DOMAIN)"; \
	KEY_DIR="data/opendkim/keys/$$DOMAIN"; \
	PRIVATE_KEY="$$KEY_DIR/$$SELECTOR.private"; \
	PUBLIC_KEY="$$KEY_DIR/$$SELECTOR.txt"; \
	echo "Adding DKIM domain: $$DOMAIN (selector: $$SELECTOR)"; \
	echo ""; \
	if [ -f "$$PRIVATE_KEY" ] && [ -f "$$PUBLIC_KEY" ]; then \
		echo "⚠ DKIM keys already exist for $$DOMAIN with selector $$SELECTOR"; \
		echo "Private key: $$PRIVATE_KEY"; \
		echo "Public key: $$PUBLIC_KEY"; \
	else \
		echo "Generating DKIM keys..."; \
		mkdir -p "$$KEY_DIR"; \
		OPENDKIM_IMAGE=$$(cd opendkim && docker build -q . 2>/dev/null || echo ''); \
		if [ -z "$$OPENDKIM_IMAGE" ]; then \
			echo "Building opendkim image..."; \
			OPENDKIM_IMAGE=$$($(DOCKER_COMPOSE) build -q opendkim 2>/dev/null | tail -1 || echo ''); \
		fi; \
		if [ -z "$$OPENDKIM_IMAGE" ]; then \
			echo "✗ Failed to build/find opendkim image"; \
			exit 1; \
		fi; \
		docker run --rm \
			-v "$$(pwd)/$$KEY_DIR:/keys" \
			-w /keys \
			$$OPENDKIM_IMAGE \
			bash -c "\
				opendkim-genkey -b 2048 -d $$DOMAIN -s $$SELECTOR -D /keys -v && \
				chmod 600 $$SELECTOR.private && \
				chmod 644 $$SELECTOR.txt" || (echo "✗ Failed to generate DKIM keys" && exit 1); \
		if [ ! -f "$$PRIVATE_KEY" ] || [ ! -f "$$PUBLIC_KEY" ]; then \
			echo "✗ DKIM key files were not created"; \
			exit 1; \
		fi; \
		echo "✓ DKIM keys generated successfully"; \
	fi; \
	echo ""; \
	echo "Updating KeyTable and SigningTable..."; \
	KEYTABLE_ENTRY="$$SELECTOR._domainkey.$$DOMAIN  $$DOMAIN:$$SELECTOR:/etc/opendkim/keys/$$DOMAIN/$$SELECTOR.private"; \
	SIGNINGTABLE_ENTRY="*@$$DOMAIN       $$SELECTOR._domainkey.$$DOMAIN"; \
	if ! grep -q "$$SELECTOR._domainkey.$$DOMAIN" opendkim/KeyTable 2>/dev/null; then \
		echo "$$KEYTABLE_ENTRY" >> opendkim/KeyTable; \
		echo "✓ Added to KeyTable"; \
	else \
		echo "⚠ KeyTable entry already exists"; \
	fi; \
	if ! grep -q "@$$DOMAIN" opendkim/SigningTable 2>/dev/null; then \
		echo "$$SIGNINGTABLE_ENTRY" >> opendkim/SigningTable; \
		echo "✓ Added to SigningTable"; \
	else \
		echo "⚠ SigningTable entry already exists"; \
	fi; \
	echo ""; \
	echo "Reloading OpenDKIM to apply changes..."; \
	$(DOCKER_COMPOSE) exec opendkim pkill -HUP opendkim || true; \
	echo ""; \
	echo "✓ Domain $$DOMAIN added successfully!"; \
	echo ""; \
	echo "Next steps:"; \
	echo "1. Add DNS TXT record: $$SELECTOR._domainkey.$$DOMAIN"; \
	echo "2. Get the DNS value with: make show-dkim DOMAIN=$$DOMAIN SELECTOR=$$SELECTOR"; \
	echo ""

show-dkim:
	@[ -n "$(DOMAIN)" ] || (echo "Usage: make show-dkim DOMAIN=example.net [SELECTOR]" && exit 1)
	@SELECTOR="$${SELECTOR:-default}"; \
	DOMAIN="$(DOMAIN)"; \
	KEY_FILE="data/opendkim/keys/$$DOMAIN/$$SELECTOR.txt"; \
	KEY_DIR="data/opendkim/keys/$$DOMAIN"; \
	echo ""; \
	echo "=== DKIM Information for $$DOMAIN ==="; \
	echo ""; \
	HAS_KEYTABLE=0; \
	HAS_SIGNINGTABLE=0; \
	if grep -q "$$SELECTOR._domainkey.$$DOMAIN" opendkim/KeyTable 2>/dev/null; then \
		HAS_KEYTABLE=1; \
	fi; \
	if grep -q "@$$DOMAIN" opendkim/SigningTable 2>/dev/null; then \
		HAS_SIGNINGTABLE=1; \
	fi; \
	if [ ! -f "$$KEY_FILE" ]; then \
		echo "✗ DKIM key not found: $$KEY_FILE"; \
		echo ""; \
		if [ "$$HAS_KEYTABLE" -eq 1 ] || [ "$$HAS_SIGNINGTABLE" -eq 1 ]; then \
			echo "⚠ Configuration exists but key file is missing!"; \
			echo ""; \
			if [ "$$HAS_KEYTABLE" -eq 1 ]; then \
				echo "KeyTable Entry:"; \
				grep "$$SELECTOR._domainkey.$$DOMAIN" opendkim/KeyTable | sed 's/^/  /'; \
				echo ""; \
			fi; \
			if [ "$$HAS_SIGNINGTABLE" -eq 1 ]; then \
				echo "SigningTable Entry:"; \
				grep "@$$DOMAIN" opendkim/SigningTable | sed 's/^/  /'; \
				echo ""; \
			fi; \
			echo "To create the missing DKIM keys, run:"; \
			echo "  make add-domain DOMAIN=$$DOMAIN SELECTOR=$$SELECTOR"; \
		else \
			echo "No DKIM configuration found for this domain."; \
			echo ""; \
			echo "To create DKIM keys for $$DOMAIN, run:"; \
			echo "  make add-domain DOMAIN=$$DOMAIN SELECTOR=$$SELECTOR"; \
		fi; \
		echo ""; \
		if [ -d "data/opendkim/keys" ] && [ "$$(ls -A data/opendkim/keys 2>/dev/null)" ]; then \
			echo "Available domains with DKIM keys:"; \
			ls -1 data/opendkim/keys/ 2>/dev/null | sed 's/^/  - /' || echo "  (none)"; \
		else \
			echo "No DKIM keys directory found or it's empty."; \
		fi; \
		echo ""; \
		exit 1; \
	fi; \
	echo "Domain: $$DOMAIN"; \
	echo "Selector: $$SELECTOR"; \
	echo ""; \
	echo "DNS Record Name:"; \
	echo "  $$SELECTOR._domainkey.$$DOMAIN"; \
	echo ""; \
	echo "DNS TXT Record Value:"; \
	PUBLIC_KEY=$$(cat "$$KEY_FILE" | grep -v '^---' | grep -v 'BEGIN' | grep -v 'END' | tr -d '\n\r ' | sed 's/^[[:space:]]*//;s/[[:space:]]*$$//'); \
	if [ -n "$$PUBLIC_KEY" ]; then \
		echo "  v=DKIM1; k=rsa; p=$$PUBLIC_KEY"; \
	else \
		echo "  ✗ (could not read public key from file)"; \
	fi; \
	echo ""; \
	echo "Key File Location:"; \
	echo "  $$KEY_FILE"; \
	echo ""; \
	if [ "$$HAS_KEYTABLE" -eq 1 ]; then \
		echo "KeyTable Entry:"; \
		grep "$$SELECTOR._domainkey.$$DOMAIN" opendkim/KeyTable | sed 's/^/  /'; \
		echo ""; \
	fi; \
	if [ "$$HAS_SIGNINGTABLE" -eq 1 ]; then \
		echo "SigningTable Entry:"; \
		grep "@$$DOMAIN" opendkim/SigningTable | sed 's/^/  /'; \
		echo ""; \
	fi; \
	echo "To add this DNS record, create a TXT record:"; \
	echo "  Name: $$SELECTOR._domainkey.$$DOMAIN"; \
	echo "  Value: (see DNS TXT Record Value above)"; \
	echo ""

reload:
	$(Q)$(DOCKER_COMPOSE) exec postfix postfix reload
	$(Q)$(DOCKER_COMPOSE) exec dovecot dovecot reload || true
	$(Q)$(DOCKER_COMPOSE) exec opendkim pkill -HUP opendkim || true

restart:
	$(Q)$(DOCKER_COMPOSE) restart

logs:
	$(Q)$(DOCKER_COMPOSE) logs -f postfix opendkim dovecot

queue-status:
	@echo "=== Email Queue Status ==="
	@$(DOCKER_COMPOSE) exec -T postfix postqueue -p || echo "Failed to query queue"

queue-flush:
	@echo "Flushing email queue..."
	@$(DOCKER_COMPOSE) exec -T postfix postqueue -f || echo "Failed to flush queue"
	@echo "✓ Queue flushed"

outbound-status:
	@echo "=== Last Outbound Email Status ==="
	@echo ""
	@echo "Recent outbound emails (last 20):"
	@$(DOCKER_COMPOSE) logs --tail=200 postfix 2>/dev/null | \
		grep -E '(status=sent|status=deferred|status=bounced|relay=.*\.com|relay=.*\.net|relay=.*\.org|to=<.*@.*>)' | \
		tail -20 | \
		sed 's/.*postfix[^|]*| //' || \
		echo "No outbound email logs found"
	@echo ""
	@echo "Summary (last 50 emails):"
	@$(DOCKER_COMPOSE) logs --tail=500 postfix 2>/dev/null | \
		grep -oE 'status=(sent|deferred|bounced)' | \
		sort | uniq -c | \
		sed 's/^/  /' || \
		echo "  No status information found"

send:
	@[ -n "$(TO)" ] || (echo "Usage: make send TO=recipient@example.com [FROM=sender@example.com] [SUBJECT=Subject]" && exit 1)
	@MAIL_DOMAIN=$$([ -f .env ] && grep "^MAIL_DOMAIN=" .env 2>/dev/null | cut -d'=' -f2 || echo "localhost"); \
	FROM="$${FROM:-postmaster@$$MAIL_DOMAIN}"; \
	SUBJECT="$${SUBJECT:-Test Email from Mailserver}"; \
	SUBMISSION_USER="$${SUBMISSION_USER:-$$FROM}"; \
	SUBMISSION_PASS="$${SUBMISSION_PASS:-}"; \
	# Try to read password from passwd file first \
	if [ -z "$$SUBMISSION_PASS" ] && [ -f data/dovecot/passwd ]; then \
		PASSWD_ENTRY=$$(grep "^$$SUBMISSION_USER:" data/dovecot/passwd 2>/dev/null || echo ""); \
		if [ -n "$$PASSWD_ENTRY" ]; then \
			PASSWD_TYPE=$$(echo "$$PASSWD_ENTRY" | cut -d':' -f2 | cut -d'}' -f1 | tr -d '{'); \
			if [ "$$PASSWD_TYPE" = "PLAIN" ]; then \
				SUBMISSION_PASS=$$(echo "$$PASSWD_ENTRY" | cut -d':' -f2- | sed 's/{PLAIN}//'); \
				echo "Using password from data/dovecot/passwd for $$SUBMISSION_USER"; \
			else \
				echo "WARNING: Password for $$SUBMISSION_USER is hashed ($$PASSWD_TYPE) in passwd file."; \
				echo "Cannot extract plain password. Falling back to .env file..."; \
			fi; \
		fi; \
	fi; \
	# Fall back to .env file if password not found in passwd \
	if [ -z "$$SUBMISSION_PASS" ] && [ -f .env ]; then \
		SUBMISSION_PASS=$$(grep "^SUBMISSION_PASS=" .env 2>/dev/null | cut -d'=' -f2- | tr -d '"' | tr -d "'" || echo ""); \
	fi; \
	# Final check \
	if [ -z "$$SUBMISSION_PASS" ]; then \
		echo "ERROR: Password not found for $$SUBMISSION_USER"; \
		echo ""; \
		echo "Options:"; \
		echo "  1. Add user with plain password: make add-user USER=$$SUBMISSION_USER PASS=yourpassword"; \
		echo "     (Note: Use {PLAIN} format in data/dovecot/passwd for SMTP auth)"; \
		echo "  2. Or set SUBMISSION_PASS=yourpassword in .env file"; \
		echo "  3. Or pass password directly: make send TO=... FROM=... SUBMISSION_PASS=..."; \
		exit 1; \
	fi; \
	echo "Sending email from $$FROM to $(TO)..."; \
	# Use swaks from postfix container to ensure TLS support \
	# Send as HTML email so it goes through pixel injection flow \
	EMAIL_BODY="<html><body><h1>Test Email</h1><p>This is a test email sent from the mailserver.</p><p><strong>Sent at:</strong> $$(date)</p><p><strong>From:</strong> $$FROM</p><p><strong>To:</strong> $(TO)</p></body></html>"; \
	if $(DOCKER_COMPOSE) exec -T postfix swaks \
		--server 127.0.0.1 \
		--port 587 \
		--from "$$FROM" \
		--to "$(TO)" \
		--auth-user "$$SUBMISSION_USER" \
		--auth-password "$$SUBMISSION_PASS" \
		--tls \
		--header "Subject: $$SUBJECT" \
		--header "Content-Type: text/html; charset=utf-8" \
		--body "$$EMAIL_BODY" 2>&1 | grep -q "queued as"; then \
		echo "✓ Email sent successfully (HTML format - will go through pixel injection)"; \
	else \
		echo "Failed to send email. Check logs with: make logs"; \
		exit 1; \
	fi

backup-dkim:
	@tar czf dkim-backup-$$\(date +%Y%m%d_%H%M%S\).tgz -C data/opendkim keys
	@echo "DKIM keys backed up to $$(ls -1 dkim-backup-*.tgz | tail -n1)"

render-maps:
	$(Q)$(DOCKER_COMPOSE) exec postfix bash -c "\
		postmap /etc/postfix/virtual_aliases && \
		postmap /etc/postfix/virtual_domains && \
		postmap /etc/postfix/vmailbox"

reports:
	@REPORTS_PORT=$${REPORTS_PORT:-8444}; \
	echo "=== Pixel Tracking Reports ==="; \
	echo ""; \
	echo "Overall Statistics:"; \
	if command -v jq >/dev/null 2>&1; then \
		curl -k -s "https://${MAIL_HOST:-localhost}:$$REPORTS_PORT/stats" 2>/dev/null | jq . 2>/dev/null || echo "  (pixelserver may not be running)"; \
	else \
		curl -k -s "https://${MAIL_HOST:-localhost}:$$REPORTS_PORT/stats" 2>/dev/null || echo "  (pixelserver may not be running)"; \
	fi; \
	echo ""; \
	echo "Recent Messages (last 20):"; \
	COUNT=0; \
	find data/pixel -name meta.json -type f -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -20 | cut -d' ' -f2- | while read f; do \
		COUNT=$$((COUNT+1)); \
		ID=$$(basename $$(dirname $$f)); \
		if command -v jq >/dev/null 2>&1; then \
			SENDER=$$(jq -r '.sender' $$f 2>/dev/null || echo "unknown"); \
			OPENED=$$(jq -r '.opened' $$f 2>/dev/null || echo "false"); \
			OPEN_COUNT=$$(jq -r '.open_count' $$f 2>/dev/null || echo "0"); \
			FIRST_OPEN=$$(jq -r '.first_open_str // "never"' $$f 2>/dev/null || echo "never"); \
		else \
			SENDER=$$(grep -o '"sender"[[:space:]]*:[[:space:]]*"[^"]*"' $$f 2>/dev/null | sed 's/.*"sender"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/' || echo "unknown"); \
			OPENED=$$(grep -o '"opened"[[:space:]]*:[[:space:]]*[^,}]*' $$f 2>/dev/null | sed 's/.*"opened"[[:space:]]*:[[:space:]]*\([^,}]*\).*/\1/' | tr -d ' ' || echo "false"); \
			OPEN_COUNT=$$(grep -o '"open_count"[[:space:]]*:[[:space:]]*[0-9]*' $$f 2>/dev/null | sed 's/.*"open_count"[[:space:]]*:[[:space:]]*\([0-9]*\).*/\1/' || echo "0"); \
			FIRST_OPEN=$$(grep -o '"first_open_str"[[:space:]]*:[[:space:]]*"[^"]*"' $$f 2>/dev/null | sed 's/.*"first_open_str"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/' || echo "never"); \
		fi; \
		echo "  $$COUNT. $$ID"; \
		echo "     Sender: $$SENDER | Opened: $$OPENED ($$OPEN_COUNT times) | First: $$FIRST_OPEN"; \
	done || echo "  No messages found"

view-reports:
	@REPORTS_PORT=$${REPORTS_PORT:-8444}; \
	echo "=== Detailed Pixel Tracking Reports ==="; \
	echo ""; \
	echo "Overall Statistics:"; \
	curl -k -s "https://${MAIL_HOST:-localhost}:$$REPORTS_PORT/stats" 2>/dev/null | jq . 2>/dev/null || echo "  (pixelserver may not be running)"; \
	echo ""; \
	echo "All Messages (JSON):"; \
	find data/pixel -name meta.json -type f | head -50 | while read f; do \
		echo ""; \
		echo "--- $$(basename $$(dirname $$f)) ---"; \
		jq '{id, sender, created_str, opened, open_count, first_open_str, last_open_str}' $$f 2>/dev/null || cat $$f; \
	done

tail-reports:
	@LOG=data/pixel/requests.log; \
	if [ -f "$$LOG" ]; then tail -n 200 "$$LOG" || true; else echo "No log file at $$LOG"; fi

clear-tracking:
	@echo "⚠ WARNING: This will delete ALL tracking data!"; \
	echo ""; \
	read -p "Are you sure? Type 'yes' to confirm: " confirm; \
	if [ "$$confirm" = "yes" ]; then \
		echo "Clearing tracking database..."; \
		find data/pixel -mindepth 1 -maxdepth 1 -type d ! -name socket -exec rm -rf {} \; 2>/dev/null || true; \
		rm -f data/pixel/requests.log 2>/dev/null || true; \
		echo "✓ Tracking database cleared"; \
	else \
		echo "Operation cancelled"; \
	fi

clear-tracking-force:
	@echo "Clearing tracking database (forced)..."; \
	find data/pixel -mindepth 1 -maxdepth 1 -type d ! -name socket -exec rm -rf {} \; 2>/dev/null || true; \
	rm -f data/pixel/requests.log 2>/dev/null || true; \
	echo "✓ Tracking database cleared"

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
	@if $(DOCKER_COMPOSE) exec -T pixelmilter timeout 1 bash -c 'echo > /dev/tcp/localhost/8892' 2>/dev/null; then \
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
	@if $(DOCKER_COMPOSE) exec -T postfix timeout 1 bash -c "echo > /dev/tcp/$${PIXEL_MILTER_IP:-172.18.0.5}/8892" 2>/dev/null; then \
		echo "✓ Postfix can connect to pixelmilter on TCP port 8892"; \
	else \
		echo "⚠ Postfix cannot connect to pixelmilter (check network connectivity)"; \
	fi
	@echo ""
	@echo "5. Checking pixelmilter process..."
	@if $(DOCKER_COMPOSE) exec -T pixelmilter pgrep -f pixelmilter >/dev/null 2>&1 || \
		($(DOCKER_COMPOSE) exec -T pixelmilter test -f /proc/1/exe 2>/dev/null && \
		$(DOCKER_COMPOSE) exec -T pixelmilter readlink /proc/1/exe 2>/dev/null | grep -q pixelmilter) || \
		$(DOCKER_COMPOSE) exec -T pixelmilter cat /proc/1/comm 2>/dev/null | grep -q pixelmilter; then \
		echo "✓ pixelmilter process is running"; \
	else \
		echo "✗ pixelmilter process not found"; \
		echo "  Checking container logs for errors..."; \
		$(DOCKER_COMPOSE) logs --tail=20 pixelmilter 2>/dev/null | tail -5 || true; \
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
	@if [ "$$(id -u)" != "0" ] 2>/dev/null && ! command -v sudo >/dev/null 2>&1; then \
		echo "⚠ Warning: Not running as root and sudo not available."; \
		echo "  Some operations may fail if root privileges are required."; \
		echo ""; \
	fi
	# Get UIDs/GIDs from running containers if available, else fallback to defaults
	if [ ! -f .container_uids.mk ]; then \
	       echo 'DOVECOT_UID:='`$(DOCKER_COMPOSE) exec -T dovecot id -u dovecot 2>/dev/null || echo 1000` > .container_uids.mk; \
	       echo 'DOVECOT_GID:='`$(DOCKER_COMPOSE) exec -T dovecot id -g dovecot 2>/dev/null || echo 1000` >> .container_uids.mk; \
	       echo 'POSTFIX_UID:='`$(DOCKER_COMPOSE) exec -T postfix id -u postfix 2>/dev/null || echo 1000` >> .container_uids.mk; \
	       echo 'POSTFIX_GID:='`$(DOCKER_COMPOSE) exec -T postfix id -g postfix 2>/dev/null || echo 1000` >> .container_uids.mk; \
	       echo 'PIXEL_UID:='`$(DOCKER_COMPOSE) exec -T pixelmilter id -u pixel 2>/dev/null || echo 1000` >> .container_uids.mk; \
	       echo 'PIXEL_GID:='`$(DOCKER_COMPOSE) exec -T pixelmilter id -g pixel 2>/dev/null || echo 1000` >> .container_uids.mk; \
	       echo 'PIXELSERVER_UID:='`$(DOCKER_COMPOSE) exec -T pixelserver id -u pixelserver 2>/dev/null || echo 1000` >> .container_uids.mk; \
	       echo 'PIXELSERVER_GID:='`$(DOCKER_COMPOSE) exec -T pixelserver id -g pixelserver 2>/dev/null || echo 1000` >> .container_uids.mk; \
	fi; \
	. ./.container_uids.mk; \
	echo "Detected UIDs/GIDs:"; \
	echo "  dovecot:   uid=$$DOVECOT_UID gid=$$DOVECOT_GID"; \
	echo "  postfix:   uid=$$POSTFIX_UID gid=$$POSTFIX_GID"; \
	echo "  pixel:     uid=$$PIXEL_UID gid=$$PIXEL_GID"; \
	echo "  pixelserver: uid=$$PIXELSERVER_UID gid=$$PIXELSERVER_GID"; \
	echo ""; \
	SUDO_CMD=$$(if [ "$$(id -u)" = "0" ] 2>/dev/null; then echo ""; elif command -v sudo >/dev/null 2>&1; then echo "sudo"; else echo ""; fi); \
	echo "Fixing data/mail ownership (dovecot)..."; \
	mkdir -p data/mail 2>/dev/null || true; \
	$$SUDO_CMD chown -R $$DOVECOT_UID:$$DOVECOT_GID data/mail 2>/dev/null || echo "  ⚠ Could not change ownership of data/mail (may need manual fix)"; \
	$$SUDO_CMD chmod -R 755 data/mail 2>/dev/null || echo "  ⚠ Could not change permissions of data/mail (may need manual fix)"; \
	echo "Fixing data/pixel ownership (pixelmilter/pixelserver)..."; \
	$$SUDO_CMD chown -R $$PIXEL_UID:$$PIXEL_GID data/pixel 2>/dev/null || echo "  ⚠ Could not change ownership of data/pixel (may need manual fix)"; \
	echo "Fixing data/logs ownership..."; \
	$$SUDO_CMD chown -R root:root data/logs 2>/dev/null || echo "  ⚠ Could not change ownership of data/logs (may need manual fix)"; \
	$$SUDO_CMD chmod 755 data/logs 2>/dev/null || true; \
	if [ -f data/logs/dovecot.log ]; then \
		$$SUDO_CMD chown $$DOVECOT_UID:$$DOVECOT_GID data/logs/dovecot.log 2>/dev/null || true; \
		$$SUDO_CMD chmod 644 data/logs/dovecot.log 2>/dev/null || true; \
	fi; \
	if [ -f data/logs/postfix.log ]; then \
		$$SUDO_CMD chown $$POSTFIX_UID:$$POSTFIX_GID data/logs/postfix.log 2>/dev/null || true; \
		$$SUDO_CMD chmod 644 data/logs/postfix.log 2>/dev/null || true; \
	fi; \
	echo "Fixing SSL certificates ownership..."; \
	# Remove ssl/key.pem and ssl/cert.pem if they are directories (should only be files)
	[ -d ssl/key.pem ] && $$SUDO_CMD rm -rf ssl/key.pem || true; \
	[ -d ssl/cert.pem ] && $$SUDO_CMD rm -rf ssl/cert.pem || true; \
	[ -d data/ssl/key.pem ] && $$SUDO_CMD rm -rf data/ssl/key.pem || true; \
	[ -d data/ssl/cert.pem ] && $$SUDO_CMD rm -rf data/ssl/cert.pem || true; \
	# Ensure host-side SSL files are owned by root so bind-mounts present root-owned files inside containers
	$$SUDO_CMD chown -R root:root ssl 2>/dev/null || echo "  ⚠ Could not change ownership of ssl (may need manual fix)"; \
	$$SUDO_CMD chmod 755 ssl 2>/dev/null || true; \
	if [ -f ssl/cert.pem ]; then \
	       $$SUDO_CMD chown root:root ssl/cert.pem 2>/dev/null || true; \
	       $$SUDO_CMD chmod 644 ssl/cert.pem 2>/dev/null || true; \
	fi; \
	if [ -f ssl/key.pem ]; then \
	       $$SUDO_CMD chown root:root ssl/key.pem 2>/dev/null || true; \
	       $$SUDO_CMD chmod 600 ssl/key.pem 2>/dev/null || true; \
	fi; \
	# Also ensure any generated certs under data/ssl are owned correctly
	if [ -d data/ssl ]; then \
		$$SUDO_CMD chown -R root:root data/ssl 2>/dev/null || true; \
		$$SUDO_CMD chmod -R 755 data/ssl 2>/dev/null || true; \
		if [ -f data/ssl/key.pem ]; then \
			$$SUDO_CMD chown root:root data/ssl/key.pem 2>/dev/null || true; \
			$$SUDO_CMD chmod 600 data/ssl/key.pem 2>/dev/null || true; \
		fi; \
	fi; \
	echo "Fixing data/dovecot/passwd permissions..."; \
	if [ -f data/dovecot/passwd ]; then \
	       $$SUDO_CMD chmod 644 data/dovecot/passwd 2>/dev/null || true; \
	       if ! $$SUDO_CMD test -r data/dovecot/passwd; then \
		       echo "  ⚠ data/dovecot/passwd is not readable, attempting to fix ownership..."; \
		       $$SUDO_CMD chown $${DOVECOT_UID:-100}:$${DOVECOT_GID:-102} data/dovecot/passwd 2>/dev/null || true; \
	       fi; \
	fi; \
	echo "Fixing postfix/resolv.conf ownership..."; \
	if [ -f postfix/resolv.conf ]; then \
		$$SUDO_CMD chown root:root postfix/resolv.conf 2>/dev/null || echo "  ⚠ Could not change ownership of postfix/resolv.conf (may need manual fix)"; \
		$$SUDO_CMD chmod 644 postfix/resolv.conf 2>/dev/null || true; \
	fi; \
	echo ""; \
	echo "✓ Ownership fixes completed!"

pull:
	@echo "Force pulling from git remote..."
	@echo ""
	@CURRENT_BRANCH=$$(git branch --show-current 2>/dev/null || echo "main"); \
	REMOTE=$$(git remote | head -1 || echo "origin"); \
	if [ -z "$$REMOTE" ]; then \
		echo "✗ No git remote found"; \
		exit 1; \
	fi; \
	echo "Current branch: $$CURRENT_BRANCH"; \
	echo "Remote: $$REMOTE"; \
	echo ""; \
	if [ -n "$$(git status --porcelain 2>/dev/null)" ]; then \
		echo "⚠ You have uncommitted changes. Stashing them..."; \
		git stash push -m "Stashed before force pull on $$(date +%Y-%m-%d\ %H:%M:%S)" || exit 1; \
		echo "✓ Changes stashed"; \
		echo ""; \
		echo "⚠ Your local changes have been stashed."; \
		echo "  To restore them later, run: git stash list"; \
		echo "  To apply them back: git stash pop"; \
		echo ""; \
	fi; \
	echo "Fetching from $$REMOTE..."; \
	git fetch $$REMOTE || exit 1; \
	echo "Resetting to $$REMOTE/$$CURRENT_BRANCH..."; \
	git reset --hard $$REMOTE/$$CURRENT_BRANCH || exit 1; \
	echo ""; \
	echo "✓ Force pull completed!"
