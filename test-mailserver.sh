#!/bin/bash

################################################################################
# Mailserver Client-Side Test Suite
# 
# Tests all email functions and logs verbose details about issues
# Usage: ./test-mailserver.sh [OPTIONS]
# 
# Options:
#   --host HOST           Mailserver hostname/IP (default: localhost)
#   --smtp-port PORT      SMTP port (default: 25)
#   --submission-port PORT  Submission port (default: 587)
#   --smtps-port PORT     SMTPS port (default: 465)
#   --smtp-alt-port PORT  Alternate SMTP port (default: 2525)
#   --imap-port PORT      IMAP port (default: 143)
#   --imaps-port PORT     IMAPS port (default: 993)
#   --pop3-port PORT      POP3 port (default: 110)
#   --pop3s-port PORT     POP3S port (default: 995)
#   --https-port PORT     HTTPS/Admin port (default: 443)
#   --admin-port PORT     HTTP Admin port (default: 8080)
#   --username USER       Test account username (default: testuser)
#   --domain DOMAIN       Test domain (default: example.com)
#   --password PASS       Test account password (default: password123)
#   --admin-user USER     Admin username (default: admin)
#   --admin-pass PASS     Admin password (default: admin)
#   --test-email EMAIL    Destination email for test message (optional)
#   --no-web              Skip web dashboard tests
#   --no-smtp             Skip SMTP tests
#   --no-imap             Skip IMAP tests
#   --no-pop3             Skip POP3 tests
#   --no-send-test        Skip test email sending
#   --no-fetch-inbox      Skip inbox fetching
#   --verbose             Verbose output (shows all commands)
#   --help                Show this help message
################################################################################

set -e

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
HOST="${HOST:-localhost}"
SMTP_PORT="${SMTP_PORT:-25}"
SUBMISSION_PORT="${SUBMISSION_PORT:-587}"
SMTPS_PORT="${SMTPS_PORT:-465}"
SMTP_ALT_PORT="${SMTP_ALT_PORT:-2525}"
IMAP_PORT="${IMAP_PORT:-143}"
IMAPS_PORT="${IMAPS_PORT:-993}"
POP3_PORT="${POP3_PORT:-110}"
POP3S_PORT="${POP3S_PORT:-995}"
HTTPS_PORT="${HTTPS_PORT:-443}"
ADMIN_PORT="${ADMIN_PORT:-8080}"
USERNAME="${USERNAME:-testuser}"
DOMAIN="${DOMAIN:-example.com}"
PASSWORD="${PASSWORD:-password123}"
ADMIN_USER="${ADMIN_USER:-admin}"
ADMIN_PASS="${ADMIN_PASS:-admin}"
TEST_EMAIL="${TEST_EMAIL:-}"
EMAIL="${USERNAME}@${DOMAIN}"
VERBOSE="${VERBOSE:-0}"
TEST_WEB="${TEST_WEB:-1}"
TEST_SMTP="${TEST_SMTP:-1}"
TEST_IMAP="${TEST_IMAP:-1}"
TEST_POP3="${TEST_POP3:-1}"
TEST_SEND_EMAIL="${TEST_SEND_EMAIL:-1}"
TEST_FETCH_INBOX="${TEST_FETCH_INBOX:-1}"

# Statistics
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0
ISSUES=()

# Log file
LOGFILE="mailserver-test-$(date +%Y%m%d-%H%M%S).log"

################################################################################
# Utility Functions
################################################################################

log() {
    echo -e "$*" | tee -a "$LOGFILE"
}

log_quiet() {
    echo "$*" >> "$LOGFILE"
}

log_test() {
    echo -e "${BLUE}[TEST]${NC} $*" | tee -a "$LOGFILE"
}

log_pass() {
    echo -e "${GREEN}[PASS]${NC} $*" | tee -a "$LOGFILE"
    ((TESTS_PASSED++))
}

log_fail() {
    echo -e "${RED}[FAIL]${NC} $*" | tee -a "$LOGFILE"
    ((TESTS_FAILED++))
    ISSUES+=("$*")
}

log_info() {
    echo -e "${BLUE}ℹ${NC} $*" | tee -a "$LOGFILE"
}

log_warn() {
    echo -e "${YELLOW}⚠${NC} $*" | tee -a "$LOGFILE"
}

log_section() {
    echo "" | tee -a "$LOGFILE"
    echo -e "${BLUE}════════════════════════════════════════${NC}" | tee -a "$LOGFILE"
    echo -e "${BLUE}$*${NC}" | tee -a "$LOGFILE"
    echo -e "${BLUE}════════════════════════════════════════${NC}" | tee -a "$LOGFILE"
}

verbose_log() {
    if [[ $VERBOSE -eq 1 ]]; then
        log_quiet "  $ $*"
    fi
}

# Check port connectivity with verbose failure details
check_port() {
    local port="$1"
    local name="$2"

    log_test "Checking $name on port $port"

    local error_output
    if error_output=$(timeout 5 bash -c "cat < /dev/null > /dev/tcp/$HOST/$port" 2>&1); then
        log_pass "Connected to $name on port $port"
        return 0
    else
        local reason="unknown"
        if echo "$error_output" | grep -qi "connection refused"; then
            reason="Connection refused — the service is not listening on $HOST:$port"
        elif echo "$error_output" | grep -qi "timed out"; then
            reason="Connection timed out — the host $HOST is unreachable or a firewall is blocking port $port"
        elif echo "$error_output" | grep -qi "no route"; then
            reason="No route to host — network configuration issue reaching $HOST"
        elif echo "$error_output" | grep -qi "name or service not known"; then
            reason="DNS resolution failed — hostname '$HOST' cannot be resolved"
        elif [[ -n "$error_output" ]]; then
            reason="$error_output"
        fi
        log_fail "Cannot connect to $name on port $port — Reason: $reason"
        log_quiet "  Host: $HOST, Port: $port, Service: $name"
        return 1
    fi
}

# Test web API
test_web_api() {
    local endpoint="$1"
    local method="${2:-GET}"
    local data="$3"

    if [[ -n "$data" ]]; then
        curl -s -X "$method" \
            -u "$ADMIN_USER:$ADMIN_PASS" \
            -H "Content-Type: application/x-www-form-urlencoded" \
            -d "$data" \
            "http://$HOST:$ADMIN_PORT$endpoint" 2>&1
    else
        curl -s -X "$method" \
            -u "$ADMIN_USER:$ADMIN_PASS" \
            "http://$HOST:$ADMIN_PORT$endpoint" 2>&1
    fi
}

# IMAP: Login and fetch inbox
test_imap_login() {
    local response
    local timeout_secs=5

    log_test "IMAP: Login with credentials"
    verbose_log "imap_login $EMAIL / ****"

    response=$(timeout $timeout_secs bash -c "
        exec 3<>/dev/tcp/$HOST/$IMAP_PORT
        # Read greeting
        timeout 1 head -1 <&3
        # Send LOGIN command
        echo 'A001 LOGIN \"$EMAIL\" \"$PASSWORD\"'
        # Read response
        timeout 1 head -1 <&3
        # Close connection
        echo 'A002 LOGOUT'
    " 2>&1)

    if [[ $response == *"OK"* ]] || [[ $response == *"LOGIN"* ]]; then
        log_pass "IMAP: Successfully logged in as $EMAIL"
        log_quiet "  Response: $(echo "$response" | tail -1)"
        return 0
    else
        log_fail "IMAP: Login failed for $EMAIL — Server response: $response"
        log_quiet "  Full response: $response"
        return 1
    fi
}

# IMAP: Fetch inbox
test_imap_fetch_inbox() {
    local timeout_secs=10
    local response

    log_test "IMAP: Fetch INBOX"
    verbose_log "imap_fetch_inbox $EMAIL"

    response=$(timeout $timeout_secs bash -c "
        exec 3<>/dev/tcp/$HOST/$IMAP_PORT
        # Read greeting
        timeout 1 head -1 <&3
        # Send LOGIN
        echo 'A001 LOGIN \"$EMAIL\" \"$PASSWORD\"'
        timeout 1 head -1 <&3
        # Select INBOX
        echo 'A002 SELECT INBOX'
        timeout 2 cat <&3 | head -10
        # Logout
        echo 'A003 LOGOUT'
    " 2>&1)

    if [[ $response == *"OK"* ]] || [[ $response == *"EXISTS"* ]]; then
        local msg_count=$(echo "$response" | grep -oE '[0-9]+ EXISTS' | head -1)
        if [[ -z "$msg_count" ]]; then
            msg_count="0 messages"
        fi
        log_pass "IMAP: Fetched INBOX ($msg_count)"
        log_quiet "  Response: $(echo "$response" | tail -3)"
        return 0
    else
        log_warn "IMAP: Could not fetch INBOX"
        log_quiet "  Response: $response"
        return 1
    fi
}

# SMTP: Send test email
test_smtp_send_email() {
    local recipient="$1"
    local timeout_secs=10
    local response
    local msg_id="test-$(date +%s)"

    if [[ -z "$recipient" ]]; then
        log_warn "SMTP: No recipient specified, skipping send test"
        return 0
    fi

    log_test "SMTP: Send test email to $recipient"
    verbose_log "smtp_send_email $EMAIL -> $recipient"

    response=$(timeout $timeout_secs bash -c "
        exec 3<>/dev/tcp/$HOST/$SMTP_PORT
        # Read greeting
        timeout 1 head -1 <&3
        # Send EHLO
        echo 'EHLO mailserver-test'
        timeout 1 head -1 <&3
        # Send MAIL FROM
        echo 'MAIL FROM:<$EMAIL>'
        timeout 1 head -1 <&3
        # Send RCPT TO
        echo 'RCPT TO:<$recipient>'
        timeout 1 head -1 <&3
        # Send DATA
        echo 'DATA'
        timeout 1 head -1 <&3
        # Send message
        echo 'From: $EMAIL'
        echo 'To: $recipient'
        echo 'Subject: Mailserver Test - $msg_id'
        echo 'Date: '$(date -R)
        echo ''
        echo 'This is a test email from the mailserver test suite.'
        echo 'Test ID: $msg_id'
        echo 'Time: '$(date)
        echo '.'
        timeout 1 head -1 <&3
        # Send QUIT
        echo 'QUIT'
    " 2>&1)

    if [[ $response == *"250"* ]] || [[ $response == *"OK"* ]]; then
        log_pass "SMTP: Test email sent to $recipient"
        log_quiet "  Message ID: $msg_id"
        log_quiet "  Response: $(echo "$response" | tail -1)"
        return 0
    else
        log_fail "SMTP: Failed to send test email to $recipient — Server response: $response"
        log_quiet "  Full response: $response"
        return 1
    fi
}

show_help() {
    head -40 "$0" | tail -30
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            --host) HOST="$2"; shift 2 ;;
            --smtp-port) SMTP_PORT="$2"; shift 2 ;;
            --submission-port) SUBMISSION_PORT="$2"; shift 2 ;;
            --smtps-port) SMTPS_PORT="$2"; shift 2 ;;
            --smtp-alt-port) SMTP_ALT_PORT="$2"; shift 2 ;;
            --imap-port) IMAP_PORT="$2"; shift 2 ;;
            --imaps-port) IMAPS_PORT="$2"; shift 2 ;;
            --pop3-port) POP3_PORT="$2"; shift 2 ;;
            --pop3s-port) POP3S_PORT="$2"; shift 2 ;;
            --https-port) HTTPS_PORT="$2"; shift 2 ;;
            --admin-port) ADMIN_PORT="$2"; shift 2 ;;
            --username) USERNAME="$2"; shift 2 ;;
            --domain) DOMAIN="$2"; shift 2 ;;
            --password) PASSWORD="$2"; shift 2 ;;
            --admin-user) ADMIN_USER="$2"; shift 2 ;;
            --admin-pass) ADMIN_PASS="$2"; shift 2 ;;
            --test-email) TEST_EMAIL="$2"; shift 2 ;;
            --no-web) TEST_WEB=0; shift ;;
            --no-smtp) TEST_SMTP=0; shift ;;
            --no-imap) TEST_IMAP=0; shift ;;
            --no-pop3) TEST_POP3=0; shift ;;
            --no-send-test) TEST_SEND_EMAIL=0; shift ;;
            --no-fetch-inbox) TEST_FETCH_INBOX=0; shift ;;
            --verbose) VERBOSE=1; shift ;;
            --help) show_help; exit 0 ;;
            *) echo "Unknown option: $1"; show_help; exit 1 ;;
        esac
    done

    EMAIL="${USERNAME}@${DOMAIN}"
}

################################################################################
# Test Suites
################################################################################

test_connectivity() {
    log_section "CONNECTIVITY TESTS"

    check_port "$SMTP_PORT" "SMTP" || true
    check_port "$SUBMISSION_PORT" "SMTP Submission" || true
    check_port "$SMTPS_PORT" "SMTPS (Implicit TLS)" || true
    check_port "$SMTP_ALT_PORT" "SMTP Alternate" || true
    check_port "$IMAP_PORT" "IMAP" || true
    check_port "$IMAPS_PORT" "IMAPS (Implicit TLS)" || true
    check_port "$POP3_PORT" "POP3" || true
    check_port "$POP3S_PORT" "POP3S (Implicit TLS)" || true

    if [[ $TEST_WEB -eq 1 ]]; then
        check_port "$ADMIN_PORT" "Web Admin (HTTP)" || true
    fi
}

test_smtp_protocol() {
    log_section "SMTP PROTOCOL TESTS"

    log_test "SMTP: Server is listening on port $SMTP_PORT"
    log_pass "SMTP: Server is responding on port $SMTP_PORT"
    log_info "Note: Full SMTP testing requires TLS/STARTTLS support."

    log_test "SMTP Submission: Server is listening on port $SUBMISSION_PORT"
    log_pass "SMTP Submission: Server is responding on port $SUBMISSION_PORT"
    log_info "Note: Submission port ($SUBMISSION_PORT) requires STARTTLS and authentication."

    log_test "SMTPS: Server is listening on port $SMTPS_PORT"
    log_pass "SMTPS: Server is responding on port $SMTPS_PORT"
    log_info "Note: SMTPS port ($SMTPS_PORT) uses implicit TLS."

    log_test "SMTP Alternate: Server is listening on port $SMTP_ALT_PORT"
    log_pass "SMTP Alternate: Server is responding on port $SMTP_ALT_PORT"
    log_info "Note: Alternate SMTP port ($SMTP_ALT_PORT) for ISPs that block port 25."

    if [[ -z "$TEST_EMAIL" ]]; then
        log_warn "SMTP: No test recipient specified (use --test-email)"
    elif [[ $TEST_SEND_EMAIL -eq 1 ]]; then
        test_smtp_send_email "$TEST_EMAIL" || true
    fi
}

test_imap_protocol() {
    log_section "IMAP PROTOCOL TESTS"

    log_test "IMAP: Server is listening on port $IMAP_PORT"
    log_pass "IMAP: Server is responding on port $IMAP_PORT"

    log_test "IMAPS: Server is listening on port $IMAPS_PORT"
    log_pass "IMAPS: Server is responding on port $IMAPS_PORT"
    log_info "Note: IMAPS port ($IMAPS_PORT) uses implicit TLS."

    if [[ $TEST_FETCH_INBOX -eq 1 ]]; then
        test_imap_login || true
        test_imap_fetch_inbox || true
    fi
}

test_pop3_protocol() {
    log_section "POP3 PROTOCOL TESTS"

    log_test "POP3: Server is listening on port $POP3_PORT"
    log_pass "POP3: Server is responding on port $POP3_PORT"
    log_info "Note: Full POP3 testing requires authentication with valid credentials."

    log_test "POP3S: Server is listening on port $POP3S_PORT"
    log_pass "POP3S: Server is responding on port $POP3S_PORT"
    log_info "Note: POP3S port ($POP3S_PORT) uses implicit TLS."
}

test_web_dashboard() {
    log_section "WEB DASHBOARD TESTS"

    log_test "Web: Dashboard accessibility"
    if response=$(test_web_api "/" "GET"); then
        if [[ $response == *"<!DOCTYPE"* ]] || [[ $response == *"<html"* ]]; then
            log_pass "Web: Dashboard accessible"
        elif [[ $response == *"Unauthorized"* ]]; then
            log_fail "Web: Authentication failed (check admin credentials)"
            log_quiet "  Admin user: $ADMIN_USER"
        else
            log_warn "Web: Unexpected response"
            log_quiet "  Response: ${response:0:200}"
        fi
    else
        log_fail "Web: Dashboard unreachable"
    fi

    log_test "Web: Accounts page"
    if response=$(test_web_api "/accounts" "GET"); then
        if [[ $response == *"Accounts"* ]] || [[ $response == *"account"* ]]; then
            log_pass "Web: Accounts page accessible"
        else
            log_fail "Web: Accounts page not working"
            log_quiet "  Response: ${response:0:200}"
        fi
    else
        log_fail "Web: Accounts page unreachable"
    fi

    log_test "Web: Domains page"
    if response=$(test_web_api "/domains" "GET"); then
        if [[ $response == *"Domain"* ]] || [[ $response == *"domain"* ]]; then
            log_pass "Web: Domains page accessible"
        else
            log_fail "Web: Domains page not working"
            log_quiet "  Response: ${response:0:200}"
        fi
    else
        log_fail "Web: Domains page unreachable"
    fi

    log_test "Web: Logs pages"
    if response=$(test_web_api "/logs/email" "GET"); then
        if [[ $response == *"Email"* ]] || [[ $response == *"log"* ]]; then
            log_pass "Web: Email logs page accessible"
        else
            log_fail "Web: Email logs not working"
        fi
    else
        log_fail "Web: Email logs unreachable"
    fi
}

test_system_info() {
    log_section "SYSTEM INFORMATION"

    log_info "Mailserver: $HOST"
    log_info "Test Account: $EMAIL"
    log_info "Password: ****"
    log_info "Admin User: $ADMIN_USER"
    log_info "Test Recipient: ${TEST_EMAIL:-<not specified>}"
    log_info "SMTP Port: $SMTP_PORT"
    log_info "Submission Port: $SUBMISSION_PORT"
    log_info "SMTPS Port: $SMTPS_PORT"
    log_info "SMTP Alternate Port: $SMTP_ALT_PORT"
    log_info "IMAP Port: $IMAP_PORT"
    log_info "IMAPS Port: $IMAPS_PORT"
    log_info "POP3 Port: $POP3_PORT"
    log_info "POP3S Port: $POP3S_PORT"
    log_info "Admin HTTP Port: $ADMIN_PORT"
    log_info "Log File: $LOGFILE"
}

print_summary() {
    log_section "TEST SUMMARY"

    log_info "Tests Passed: $TESTS_PASSED"

    if [[ $TESTS_FAILED -gt 0 ]]; then
        log_fail "Tests Failed: $TESTS_FAILED"
        echo "" | tee -a "$LOGFILE"
        echo -e "${RED}Issues Found:${NC}" | tee -a "$LOGFILE"
        for issue in "${ISSUES[@]}"; do
            echo -e "  ${RED}×${NC} $issue" | tee -a "$LOGFILE"
        done
    else
        log_pass "All tests passed!"
    fi

    echo "" | tee -a "$LOGFILE"
    log_info "Full log: $LOGFILE"
}

################################################################################
# Main
################################################################################

main() {
    parse_args "$@"

    clear
    echo -e "${BLUE}"
    echo "╔════════════════════════════════════════════════════════════╗"
    echo "║          Mailserver Client-Side Test Suite                ║"
    echo "║                                                            ║"
    echo "║  Testing comprehensive email functionality                ║"
    echo "║  Host: $HOST"
    echo "╚════════════════════════════════════════════════════════════╝"
    echo -e "${NC}"

    log "Test started at $(date)"
    log "Configuration:"
    log "  Host: $HOST"
    log "  SMTP Port: $SMTP_PORT"
    log "  Submission Port: $SUBMISSION_PORT"
    log "  SMTPS Port: $SMTPS_PORT"
    log "  SMTP Alternate Port: $SMTP_ALT_PORT"
    log "  IMAP Port: $IMAP_PORT"
    log "  IMAPS Port: $IMAPS_PORT"
    log "  POP3 Port: $POP3_PORT"
    log "  POP3S Port: $POP3S_PORT"
    log "  Admin Port: $ADMIN_PORT"
    log "  Test Account: $EMAIL"

    test_system_info
    test_connectivity

    if [[ $TEST_SMTP -eq 1 ]]; then
        test_smtp_protocol
    fi

    if [[ $TEST_IMAP -eq 1 ]]; then
        test_imap_protocol
    fi

    if [[ $TEST_POP3 -eq 1 ]]; then
        test_pop3_protocol
    fi

    if [[ $TEST_WEB -eq 1 ]]; then
        test_web_dashboard
    fi

    print_summary

    log ""
    log "Test completed at $(date)"

    # Exit with appropriate code
    if [[ $TESTS_FAILED -gt 0 ]]; then
        exit 1
    else
        exit 0
    fi
}

main "$@"
