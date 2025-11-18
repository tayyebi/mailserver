#!/usr/bin/env python3
"""
Postfix content filter reinjection script
Sends filtered email via SMTP to reinjection service on port 10025
"""
import sys
import smtplib
import email
import email.policy
from email import message_from_bytes

def extract_email_address(header_value):
    """Extract email address from header value, handling 'Name <email>' format."""
    header_value = header_value.strip()
    # Match email in angle brackets: Name <email@domain.com>
    import re
    match = re.search(r'<([^>]+)>', header_value)
    if match:
        return match.group(1)
    # If no angle brackets, use the whole value
    return header_value.strip()

def main():
    # Read email from stdin
    email_data = sys.stdin.buffer.read()
    
    # Parse email to extract headers
    try:
        msg = message_from_bytes(email_data, policy=email.policy.default)
    except Exception as e:
        sys.stderr.write(f'ERROR: Failed to parse email: {e}\n')
        sys.exit(1)
    
    # Extract sender and recipient
    from_addr = None
    to_addr = None
    
    if msg['From']:
        from_addr = extract_email_address(msg['From'])
    if msg['To']:
        to_addr = extract_email_address(msg['To'])
    
    if not to_addr:
        sys.stderr.write('ERROR: No To header found\n')
        sys.exit(1)
    
    if not from_addr:
        from_addr = 'noreply@localhost'
    
    # Connect to reinjection service using smtplib
    try:
        # Use SMTP class directly (not SMTP_SSL) since port 10025 doesn't use TLS
        smtp = smtplib.SMTP('127.0.0.1', 10025, timeout=30)
        smtp.set_debuglevel(0)  # Set to 1 for debugging
        
        # Send email using send_message which handles all SMTP protocol details
        smtp.send_message(msg, from_addr=from_addr, to_addrs=[to_addr])
        smtp.quit()
        
    except smtplib.SMTPException as e:
        sys.stderr.write(f'ERROR: SMTP error: {e}\n')
        sys.exit(1)
    except Exception as e:
        sys.stderr.write(f'ERROR: Unexpected error: {e}\n')
        sys.exit(1)

if __name__ == '__main__':
    main()

