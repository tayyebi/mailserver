#!/usr/bin/env python3
"""
Postfix content filter reinjection script
Sends filtered email via SMTP to reinjection service on port 10025
"""
import sys
import socket

def main():
    # Read email from stdin
    email = sys.stdin.read()
    
    # Extract recipient from To header
    to_addr = None
    for line in email.split('\n'):
        if line.lower().startswith('to:'):
            to_addr = line.split(':', 1)[1].strip()
            # Remove angle brackets if present
            to_addr = to_addr.strip('<>')
            break
    
    if not to_addr:
        sys.stderr.write('ERROR: No To header found\n')
        sys.exit(1)
    
    # Connect to reinjection service
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.connect(('127.0.0.1', 10025))
    except Exception as e:
        sys.stderr.write(f'ERROR: Failed to connect to 127.0.0.1:10025: {e}\n')
        sys.exit(1)
    
    try:
        # Send SMTP commands
        sock.sendall(b'EHLO localhost\r\n')
        response = sock.recv(1024)
        if not response.startswith(b'250'):
            sys.stderr.write(f'ERROR: EHLO failed: {response}\n')
            sys.exit(1)
        
        sock.sendall(b'MAIL FROM:<noreply@localhost>\r\n')
        response = sock.recv(1024)
        if not response.startswith(b'250'):
            sys.stderr.write(f'ERROR: MAIL FROM failed: {response}\n')
            sys.exit(1)
        
        sock.sendall(f'RCPT TO:<{to_addr}>\r\n'.encode())
        response = sock.recv(1024)
        if not response.startswith(b'250'):
            sys.stderr.write(f'ERROR: RCPT TO failed: {response}\n')
            sys.exit(1)
        
        sock.sendall(b'DATA\r\n')
        response = sock.recv(1024)
        if not response.startswith(b'354'):
            sys.stderr.write(f'ERROR: DATA failed: {response}\n')
            sys.exit(1)
        
        # Send email content
        sock.sendall(email.encode())
        sock.sendall(b'\r\n.\r\n')
        response = sock.recv(1024)
        if not response.startswith(b'250'):
            sys.stderr.write(f'ERROR: Message send failed: {response}\n')
            sys.exit(1)
        
        sock.sendall(b'QUIT\r\n')
        sock.close()
    except Exception as e:
        sys.stderr.write(f'ERROR: SMTP error: {e}\n')
        sys.exit(1)

if __name__ == '__main__':
    main()

