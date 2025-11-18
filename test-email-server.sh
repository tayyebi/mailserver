#!/bin/bash
# Email Server Test Script
# Comprehensive testing of SMTP, IMAP, and POP3 using telnet and other command-line tools
# Usage: ./test-email-server.sh
# Or: SERVER=mail.example.com USER=user@example.com PASS=password TEST_EMAIL=test@example.com ./test-email-server.sh

set -euo pipefail

# Configuration with defaults
SERVER="${SERVER:-mail.gordarg.com}"
USER="${USER:-postmaster@gordarg.com}"
PASS="${PASS:-P@ssw0rd123!@#3.14}"
TEST_EMAIL="${TEST_EMAIL:-tayyebimohammadreza@gmail.com}"
VERBOSE="${VERBOSE:-0}"
IGNORE_CERT="${IGNORE_CERT:-0}"  # Set to 1 to ignore certificate verification errors

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
NC='\033[0m' # No Color
BOLD='\033[1m'

# Test statistics
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_WARNED=0
START_TIME=$(date +%s)

# Helper functions
print_header() {
    echo -e "\n${BLUE}${BOLD}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}${BOLD}  $1${NC}"
    echo -e "${BLUE}${BOLD}═══════════════════════════════════════════════════════════${NC}\n"
}

print_success() {
    echo -e "${GREEN}✓ $1${NC}"
    ((TESTS_PASSED++)) || true
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
    ((TESTS_FAILED++)) || true
}

print_warning() {
    echo -e "${YELLOW}⚠ $1${NC}"
    ((TESTS_WARNED++)) || true
}

print_info() {
    echo -e "${CYAN}ℹ $1${NC}"
}

print_detail() {
    if [ "$VERBOSE" = "1" ]; then
        echo -e "${MAGENTA}  → $1${NC}"
    fi
}

# Measure response time
measure_time() {
    local start=$(date +%s%N)
    "$@"
    local end=$(date +%s%N)
    local duration=$(( (end - start) / 1000000 ))
    echo "$duration"
}

# Test port connectivity with timeout
test_port() {
    local host=$1
    local port=$2
    local timeout=${3:-3}
    local start_time=$(date +%s%N)
    
    if timeout $timeout bash -c "echo > /dev/tcp/$host/$port" 2>/dev/null; then
        local end_time=$(date +%s%N)
        local duration=$(( (end_time - start_time) / 1000000 ))
        print_detail "Connection time: ${duration}ms"
        return 0
    else
        return 1
    fi
}

# Generate AUTH PLAIN base64 string
generate_auth_plain() {
    local username=$1
    local password=$2
    
    # Try Python3 first (most reliable)
    if command -v python3 >/dev/null 2>&1; then
        python3 <<EOF
import base64
import sys
auth_string = b'\x00' + b'$username' + b'\x00' + b'$password'
print(base64.b64encode(auth_string).decode('ascii'))
EOF
        return 0
    fi
    
    # Fallback to printf + base64
    printf '\0%s\0%s' "$username" "$password" | base64 -w 0 2>/dev/null || \
    printf '\0%s\0%s' "$username" "$password" | base64 | tr -d '\n'
}

# Test DNS resolution and records
test_dns() {
    print_header "DNS Resolution Tests"
    
    local dns_ok=true
    
    # A record
    if host "$SERVER" >/dev/null 2>&1; then
        print_success "DNS A record resolves for $SERVER"
        local ip=$(host "$SERVER" | grep "has address" | head -1 | awk '{print $4}')
        if [ -n "$ip" ]; then
            print_info "IP Address: $ip"
            print_detail "$(host "$SERVER" | head -3)"
        fi
    else
        print_error "DNS A record resolution failed for $SERVER"
        dns_ok=false
    fi
    
    # MX record
    local domain=$(echo "$SERVER" | sed 's/^[^.]*\.//')
    if host -t MX "$domain" >/dev/null 2>&1; then
        print_success "MX record found for $domain"
        print_detail "$(host -t MX "$domain" | head -5)"
    else
        print_warning "No MX record found for $domain (may be intentional)"
    fi
    
    # PTR record (reverse DNS)
    if [ -n "${ip:-}" ]; then
        if host "$ip" >/dev/null 2>&1; then
            print_success "Reverse DNS (PTR) resolves"
            print_detail "$(host "$ip" | head -2)"
        else
            print_warning "Reverse DNS (PTR) not configured"
        fi
    fi
    
    # SPF record
    if host -t TXT "$domain" >/dev/null 2>&1; then
        local spf=$(host -t TXT "$domain" 2>/dev/null | grep -i "v=spf1" || true)
        if [ -n "$spf" ]; then
            print_success "SPF record found"
            print_detail "$spf"
        else
            print_warning "No SPF record found (recommended for email deliverability)"
        fi
    fi
    
    return 0
}

# Test TLS/SSL certificate
test_tls_certificate() {
    local port=$1
    local starttls=${2:-""}
    
    print_info "Checking TLS certificate on port $port..."
    
    local cmd="openssl s_client -connect $SERVER:$port -servername $SERVER"
    if [ -n "$starttls" ]; then
        cmd="$cmd -starttls $starttls"
    fi
    
    local cert_info=$(echo "QUIT" | timeout 10 $cmd 2>&1 | grep -A 20 "Certificate chain\|Verify return code" || true)
    
    if echo "$cert_info" | grep -q "Verify return code: 0"; then
        print_success "Certificate is valid"
    elif echo "$cert_info" | grep -q "Verify return code"; then
        local code=$(echo "$cert_info" | grep "Verify return code" | awk '{print $4}')
        print_warning "Certificate verification code: $code"
    fi
    
    # Get certificate expiration
    local expiry=$(echo "QUIT" | timeout 10 $cmd 2>&1 | openssl x509 -noout -dates 2>/dev/null | grep "notAfter" | cut -d= -f2 || true)
    if [ -n "$expiry" ]; then
        print_info "Certificate expires: $expiry"
    fi
    
    # Get certificate subject
    local subject=$(echo "QUIT" | timeout 10 $cmd 2>&1 | openssl x509 -noout -subject 2>/dev/null | cut -d= -f2- || true)
    if [ -n "$subject" ]; then
        print_detail "Certificate subject: $subject"
    fi
}

# Test SMTP port 25 (standard SMTP)
test_smtp_25() {
    print_header "SMTP Port 25 (Standard SMTP)"
    
    if ! test_port "$SERVER" 25; then
        print_error "Cannot connect to $SERVER:25"
        return 1
    fi
    
    print_success "Port 25 is reachable"
    
    print_info "Testing SMTP banner and EHLO..."
    local response=$( {
        echo "EHLO test.local"
        sleep 1
        echo "QUIT"
    } | timeout 10 telnet "$SERVER" 25 2>/dev/null || true)
    
    if echo "$response" | grep -q "250"; then
        print_success "SMTP server responding"
        if [ "$VERBOSE" = "1" ]; then
            echo "$response" | grep "250" | head -5 | sed 's/^/  /'
        fi
    else
        print_error "SMTP banner not received"
        if [ "$VERBOSE" = "1" ]; then
            echo "$response" | head -10 | sed 's/^/  /'
        fi
    fi
    
    # Check for STARTTLS support
    if echo "$response" | grep -qi "STARTTLS"; then
        print_info "STARTTLS is supported on port 25"
    fi
}

# Test SMTP port 587 (Submission with STARTTLS)
test_smtp_587() {
    print_header "SMTP Port 587 (Submission with STARTTLS)"
    
    if ! test_port "$SERVER" 587; then
        print_error "Cannot connect to $SERVER:587"
        return 1
    fi
    
    print_success "Port 587 is reachable"
    
    # Test plain connection first
    print_info "Testing plain connection..."
    local plain_response=$( {
        echo "EHLO test.local"
        sleep 1
        echo "QUIT"
    } | timeout 10 telnet "$SERVER" 587 2>/dev/null || true)
    
    if echo "$plain_response" | grep -q "250\|220"; then
        print_success "SMTP server responding on port 587"
    fi
    
    # Test STARTTLS
    print_info "Testing STARTTLS..."
    local starttls_response=$( {
        echo "EHLO test.local"
        sleep 1
        echo "STARTTLS"
        sleep 1
        echo "QUIT"
    } | timeout 10 telnet "$SERVER" 587 2>/dev/null || true)
    
    if echo "$starttls_response" | grep -q "220\|250.*STARTTLS"; then
        print_success "STARTTLS command accepted"
    else
        print_warning "STARTTLS test incomplete (may require TLS connection)"
    fi
    
    # Test with OpenSSL STARTTLS
    print_info "Testing STARTTLS with OpenSSL..."
    local openssl_opts="-connect $SERVER:587 -starttls smtp -servername $SERVER"
    [ "$IGNORE_CERT" = "1" ] && openssl_opts="$openssl_opts"  # OpenSSL continues by default even with cert errors
    local ssl_response=$(echo "EHLO test.local" | timeout 10 openssl s_client $openssl_opts 2>&1 || true)
    
    if echo "$ssl_response" | grep -q "250"; then
        print_success "STARTTLS works via OpenSSL"
        
        # Check EHLO capabilities
        if echo "$ssl_response" | grep -qi "AUTH"; then
            print_info "AUTH mechanisms supported:"
            echo "$ssl_response" | grep -i "AUTH" | sed 's/^/  /'
        fi
        
        # Test TLS certificate
        test_tls_certificate 587 smtp
    else
        print_warning "STARTTLS test failed"
        if [ "$VERBOSE" = "1" ]; then
            echo "$ssl_response" | grep -iE "error|fail|handshake" | head -5 | sed 's/^/  /'
        fi
    fi
}

# Test SMTP port 465 (SMTPS with TLS wrapper)
test_smtp_465() {
    print_header "SMTP Port 465 (SMTPS with TLS)"
    
    if ! test_port "$SERVER" 465; then
        print_error "Cannot connect to $SERVER:465"
        return 1
    fi
    
    print_success "Port 465 is reachable"
    
    print_info "Testing TLS connection..."
    local response=$(echo "EHLO test.local" | timeout 10 openssl s_client -connect "$SERVER:465" -servername "$SERVER" 2>&1 || true)
    
    if echo "$response" | grep -q "250"; then
        print_success "TLS connection works"
        
        # Check EHLO capabilities
        if echo "$response" | grep -qi "AUTH"; then
            print_info "AUTH mechanisms supported:"
            echo "$response" | grep -i "AUTH" | sed 's/^/  /'
        fi
        
        # Test TLS certificate
        test_tls_certificate 465
    else
        print_warning "TLS connection test failed"
        if [ "$VERBOSE" = "1" ]; then
            echo "$response" | grep -iE "error|fail|handshake" | head -5 | sed 's/^/  /'
        fi
    fi
}

# Test SMTP Authentication (AUTH PLAIN and AUTH LOGIN)
test_smtp_auth() {
    print_header "SMTP Authentication Tests"
    
    print_info "Username: $USER"
    print_info "Testing on port 587..."
    
    # Generate AUTH PLAIN credentials
    local auth_plain=$(generate_auth_plain "$USER" "$PASS")
    
    if [ -z "$auth_plain" ]; then
        print_error "Failed to generate AUTH PLAIN credentials"
        return 1
    fi
    
    print_info "Attempting AUTH PLAIN..."
    
    # Test authentication
    local openssl_opts="-connect $SERVER:587 -starttls smtp -servername $SERVER"
    # OpenSSL continues by default even with certificate errors, just prints warnings
    local response=$( {
        echo "EHLO test.local"
        sleep 1
        echo "AUTH PLAIN $auth_plain"
        sleep 2
        echo "QUIT"
    } | timeout 15 openssl s_client $openssl_opts 2>&1 || true)
    
    if echo "$response" | grep -q "235"; then
        print_success "AUTH PLAIN authentication successful!"
        print_detail "Server response: $(echo "$response" | grep "235" | head -1)"
    else
        print_error "AUTH PLAIN authentication failed"
        local error=$(echo "$response" | grep -iE "535|530|454|500|501|504|auth.*fail|error" | head -3)
        if [ -n "$error" ]; then
            echo "$error" | sed 's/^/  /'
        fi
        if [ "$VERBOSE" = "1" ]; then
            echo "$response" | tail -20 | sed 's/^/  /'
        fi
    fi
    
    # Test AUTH LOGIN if supported
    print_info "Testing AUTH LOGIN..."
    local user_b64=$(echo -n "$USER" | base64 -w 0 2>/dev/null || echo -n "$USER" | base64 | tr -d '\n')
    local pass_b64=$(echo -n "$PASS" | base64 -w 0 2>/dev/null || echo -n "$PASS" | base64 | tr -d '\n')
    
    local openssl_opts="-connect $SERVER:587 -starttls smtp -servername $SERVER"
    # OpenSSL continues by default even with certificate errors
    local login_response=$( {
        echo "EHLO test.local"
        sleep 1
        echo "AUTH LOGIN"
        sleep 1
        echo "$user_b64"
        sleep 1
        echo "$pass_b64"
        sleep 2
        echo "QUIT"
    } | timeout 15 openssl s_client $openssl_opts 2>&1 || true)
    
    if echo "$login_response" | grep -q "235"; then
        print_success "AUTH LOGIN authentication successful!"
    else
        print_info "AUTH LOGIN not supported or failed (this is normal if AUTH PLAIN works)"
    fi
}

# Send test email via SMTP
send_test_email() {
    print_header "Sending Test Email"
    
    print_info "From: $USER"
    print_info "To: $TEST_EMAIL"
    
    # Generate AUTH PLAIN credentials
    local auth_plain=$(generate_auth_plain "$USER" "$PASS")
    
    if [ -z "$auth_plain" ]; then
        print_error "Failed to generate AUTH PLAIN credentials"
        return 1
    fi
    
    print_info "Connecting to SMTP server and sending email..."
    
    local email_id="test-$(date +%s)"
    local openssl_opts="-connect $SERVER:587 -starttls smtp -servername $SERVER"
    # OpenSSL continues by default even with certificate errors
    local response=$( {
        echo "EHLO test.local"
        sleep 1
        echo "AUTH PLAIN $auth_plain"
        sleep 2
        echo "MAIL FROM:<$USER>"
        sleep 1
        echo "RCPT TO:<$TEST_EMAIL>"
        sleep 1
        echo "DATA"
        sleep 1
        echo "From: $USER"
        echo "To: $TEST_EMAIL"
        echo "Subject: Test Email from $SERVER - $email_id"
        echo "Message-ID: <$email_id@$SERVER>"
        echo "Date: $(date -R)"
        echo ""
        echo "This is a test email sent from $SERVER"
        echo ""
        echo "Test Details:"
        echo "- Server: $SERVER"
        echo "- Timestamp: $(date)"
        echo "- Test ID: $email_id"
        echo ""
        echo "If you receive this email, your SMTP server is working correctly!"
        echo "."
        sleep 2
        echo "QUIT"
    } | timeout 30 openssl s_client $openssl_opts 2>&1 || true)
    
    if echo "$response" | grep -q "250.*Ok\|250.*queued\|354"; then
        print_success "Email sent successfully!"
        print_info "Message ID: $email_id"
        echo "$response" | grep -E "250|354" | tail -3 | sed 's/^/  /'
        print_info "Check $TEST_EMAIL inbox (and spam folder) for the test email"
    else
        print_error "Failed to send email"
        local error=$(echo "$response" | grep -iE "error|fail|reject|deny|550|553|554" | head -5)
        if [ -n "$error" ]; then
            echo "$error" | sed 's/^/  /'
        fi
        if [ "$VERBOSE" = "1" ]; then
            echo "$response" | tail -30 | sed 's/^/  /'
        fi
    fi
}

# Test IMAP port 143
test_imap_143() {
    print_header "IMAP Port 143 (Plain)"
    
    if ! test_port "$SERVER" 143; then
        print_error "Cannot connect to $SERVER:143"
        return 1
    fi
    
    print_success "Port 143 is reachable"
    
    print_info "Testing IMAP connection..."
    local response=$( {
        echo "a1 CAPABILITY"
        sleep 1
        echo "a2 LOGOUT"
    } | timeout 10 telnet "$SERVER" 143 2>/dev/null || true)
    
    if echo "$response" | grep -q "OK"; then
        print_success "IMAP server responding"
        if echo "$response" | grep -qi "STARTTLS\|LOGINDISABLED"; then
            print_warning "Plain IMAP may require STARTTLS (use port 993 for secure connection)"
        fi
    else
        print_warning "IMAP test incomplete"
    fi
}

# Test IMAP port 993 (IMAPS)
test_imap_993() {
    print_header "IMAP Port 993 (IMAPS)"
    
    if ! test_port "$SERVER" 993; then
        print_error "Cannot connect to $SERVER:993"
        return 1
    fi
    
    print_success "Port 993 is reachable"
    
    print_info "Testing IMAPS connection..."
    local response=$( {
        echo "a1 CAPABILITY"
        sleep 1
        echo "a2 LOGOUT"
    } | timeout 10 openssl s_client -connect "$SERVER:993" -servername "$SERVER" -quiet 2>&1 || true)
    
    if echo "$response" | grep -q "OK"; then
        print_success "IMAPS connection works"
        
        # Show capabilities
        if echo "$response" | grep -qi "CAPABILITY"; then
            print_info "IMAP capabilities:"
            echo "$response" | grep -i "CAPABILITY" | sed 's/^/  /' | head -3
        fi
        
        # Test TLS certificate
        test_tls_certificate 993
    else
        print_warning "IMAPS test incomplete"
    fi
}

# Test IMAP Authentication and retrieve mailbox info
test_imap_auth() {
    print_header "IMAP Authentication and Mailbox Tests"
    
    print_info "Username: $USER"
    
    print_info "Testing IMAP login on port 993..."
    local response=$( {
        echo "a1 LOGIN \"$USER\" \"$PASS\""
        sleep 2
        echo "a2 LIST \"\" \"*\""
        sleep 1
        echo "a3 STATUS \"INBOX\" (MESSAGES RECENT UNSEEN)"
        sleep 1
        echo "a4 LOGOUT"
    } | timeout 15 openssl s_client -connect "$SERVER:993" -servername "$SERVER" -quiet 2>&1 || true)
    
    if echo "$response" | grep -q "OK.*LOGIN"; then
        print_success "IMAP authentication successful!"
        
        # Show mailbox list
        if echo "$response" | grep -qi "LIST"; then
            print_info "Available mailboxes:"
            echo "$response" | grep -i "LIST" | head -10 | sed 's/^/  /'
        fi
        
        # Show INBOX status
        if echo "$response" | grep -qi "STATUS"; then
            print_info "INBOX status:"
            echo "$response" | grep -i "STATUS" | sed 's/^/  /'
        fi
    else
        print_error "IMAP authentication failed"
        local error=$(echo "$response" | grep -iE "NO|BAD|error|fail" | head -3)
        if [ -n "$error" ]; then
            echo "$error" | sed 's/^/  /'
        fi
    fi
}

# Test POP3 port 110
test_pop3_110() {
    print_header "POP3 Port 110 (Plain)"
    
    if ! test_port "$SERVER" 110; then
        print_error "Cannot connect to $SERVER:110"
        return 1
    fi
    
    print_success "Port 110 is reachable"
    
    print_info "Testing POP3 connection..."
    local response=$( {
        echo "QUIT"
    } | timeout 10 telnet "$SERVER" 110 2>/dev/null || true)
    
    if echo "$response" | grep -q "OK\|+OK"; then
        print_success "POP3 server responding"
        print_warning "Plain POP3 is insecure (use port 995 for secure connection)"
    else
        print_warning "POP3 test incomplete"
    fi
}

# Test POP3 port 995 (POP3S)
test_pop3_995() {
    print_header "POP3 Port 995 (POP3S)"
    
    if ! test_port "$SERVER" 995; then
        print_error "Cannot connect to $SERVER:995"
        return 1
    fi
    
    print_success "Port 995 is reachable"
    
    print_info "Testing POP3S connection..."
    local response=$( {
        echo "QUIT"
    } | timeout 10 openssl s_client -connect "$SERVER:995" -servername "$SERVER" -quiet 2>&1 || true)
    
    if echo "$response" | grep -q "OK\|+OK"; then
        print_success "POP3S connection works"
        
        # Test TLS certificate
        test_tls_certificate 995
    else
        print_warning "POP3S test incomplete"
    fi
}

# Test POP3 Authentication and retrieve mailbox stats
test_pop3_auth() {
    print_header "POP3 Authentication and Mailbox Tests"
    
    print_info "Username: $USER"
    
    print_info "Testing POP3 login on port 995..."
    local response=$( {
        echo "USER $USER"
        sleep 1
        echo "PASS $PASS"
        sleep 2
        echo "STAT"
        sleep 1
        echo "LIST"
        sleep 1
        echo "QUIT"
    } | timeout 15 openssl s_client -connect "$SERVER:995" -servername "$SERVER" -quiet 2>&1 || true)
    
    if echo "$response" | grep -q "+OK.*STAT"; then
        print_success "POP3 authentication successful!"
        
        # Show STAT response (message count and size)
        if echo "$response" | grep -q "+OK.*STAT"; then
            print_info "Mailbox statistics:"
            echo "$response" | grep "+OK.*STAT" | sed 's/^/  /'
        fi
    else
        print_error "POP3 authentication failed"
        local error=$(echo "$response" | grep -iE "-ERR|error|fail" | head -3)
        if [ -n "$error" ]; then
            echo "$error" | sed 's/^/  /'
        fi
    fi
}

# Print summary statistics
print_summary() {
    print_header "Test Summary"
    
    local end_time=$(date +%s)
    local duration=$((end_time - START_TIME))
    
    echo -e "${BOLD}Test Results:${NC}"
    echo -e "  ${GREEN}Passed:${NC} $TESTS_PASSED"
    echo -e "  ${RED}Failed:${NC} $TESTS_FAILED"
    echo -e "  ${YELLOW}Warnings:${NC} $TESTS_WARNED"
    echo -e "  ${CYAN}Duration:${NC} ${duration}s"
    echo ""
    
    if [ $TESTS_FAILED -eq 0 ]; then
        print_success "All critical tests passed!"
    else
        print_error "Some tests failed. Check the output above for details."
    fi
    
    if [ $TESTS_WARNED -gt 0 ]; then
        print_info "Some warnings were issued. These may not be critical but should be reviewed."
    fi
}

# Main test function
main() {
    echo -e "${BLUE}${BOLD}"
    echo "╔═══════════════════════════════════════════════════════════╗"
    echo "║         Email Server Comprehensive Test Suite            ║"
    echo "╚═══════════════════════════════════════════════════════════╝"
    echo -e "${NC}"
    
    print_info "Server: ${BOLD}$SERVER${NC}"
    print_info "User: ${BOLD}$USER${NC}"
    print_info "Test Email: ${BOLD}$TEST_EMAIL${NC}"
    print_info "Verbose Mode: ${BOLD}$VERBOSE${NC}"
    print_info "Ignore Cert Errors: ${BOLD}$IGNORE_CERT${NC}"
    if [ "$IGNORE_CERT" = "1" ]; then
        print_warning "Certificate verification is disabled (for testing only)"
    fi
    echo ""
    
    # Check required tools
    print_header "Checking Required Tools"
    local missing_tools=()
    for tool in telnet openssl base64 host python3; do
        if command -v "$tool" >/dev/null 2>&1; then
            print_success "$tool is installed"
        else
            print_error "$tool is not installed"
            missing_tools+=("$tool")
        fi
    done
    
    if [ ${#missing_tools[@]} -gt 0 ]; then
        print_warning "Missing tools: ${missing_tools[*]}"
        print_info "Install with: sudo apt-get install ${missing_tools[*]}"
    fi
    
    # Run tests
    test_dns
    test_smtp_25
    test_smtp_587
    test_smtp_465
    test_smtp_auth
    send_test_email
    test_imap_143
    test_imap_993
    test_imap_auth
    test_pop3_110
    test_pop3_995
    test_pop3_auth
    
    # Print summary
    print_summary
    
    echo ""
    print_info "For verbose output, run: VERBOSE=1 ./test-email-server.sh"
    print_info "To customize settings, use environment variables:"
    print_info "  SERVER=mail.example.com USER=user@example.com PASS=password TEST_EMAIL=test@example.com ./test-email-server.sh"
}

# Handle script arguments
if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    echo "Email Server Test Script"
    echo ""
    echo "Usage: ./test-email-server.sh [OPTIONS]"
    echo ""
    echo "Environment Variables:"
    echo "  SERVER      Mail server hostname (default: mail.gordarg.com)"
    echo "  USER        Username for authentication (default: postmaster@gordarg.com)"
    echo "  PASS        Password for authentication"
    echo "  TEST_EMAIL  Email address to send test email to"
    echo "  VERBOSE     Set to 1 for verbose output"
    echo "  IGNORE_CERT Set to 1 to ignore certificate verification errors"
    echo ""
    echo "Examples:"
    echo "  ./test-email-server.sh"
    echo "  SERVER=mail.example.com USER=user@example.com PASS=secret ./test-email-server.sh"
    echo "  VERBOSE=1 ./test-email-server.sh"
    echo "  IGNORE_CERT=1 ./test-email-server.sh  # For self-signed certificates"
    exit 0
fi

# Run main function
main
