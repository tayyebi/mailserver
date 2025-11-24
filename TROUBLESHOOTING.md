# Troubleshooting & Diagnostics Log

This document records the troubleshooting steps, diagnostic commands, and fixes applied to the Mailserver project, specifically focusing on the `postfix-reinject` and `pixelmilter` integration.

## 1. Issue: Lost Bcc Recipients in Content Filter

**Symptom:** Emails sent via the content filter were losing `Bcc` recipients.
**Root Cause:** The `postfix-reinject` binary was parsing email headers to determine recipients. Since `Bcc` headers are stripped before transmission (or not present in the headers passed to the filter), these recipients were dropped.
**Fix:**
1.  Updated `postfix-reinject` (Rust) to accept `--sender` and `--recipient` command-line arguments.
2.  Updated Postfix `master.cf` to pass the envelope sender and recipient macros (`${sender}`, `${recipient}`) to the content filter script.
3.  Updated the content filter script to pass these arguments to the `postfix-reinject` binary.

## 2. Diagnostic Commands (Remote)

These commands are useful for diagnosing issues on the remote SMTP server (`smtp-uk`).

### Service Status & Logs
```bash
# Check status of all relevant services
ssh smtp-uk "systemctl status postfix pixelmilter pixelserver"

# Follow logs for all services (live tail)
ssh smtp-uk "journalctl -u postfix -u pixelmilter -u pixelserver -f"

# Check Postfix logs specifically for a recent timeframe
ssh smtp-uk "journalctl -u postfix --since '10 minutes ago'"

# Search for a specific email address in logs
ssh smtp-uk "journalctl | grep 'user@example.com'"
```

### Mail Queue & Delivery
```bash
# Check the Postfix mail queue
ssh smtp-uk "mailq"

# Flush the queue (force delivery attempt)
ssh smtp-uk "postqueue -f"

# Check for specific Queue ID
ssh smtp-uk "postcat -q <QUEUE_ID>"
```

### Testing Email Injection
If `swaks` is not available, use `sendmail` to inject a test email with HTML content (to trigger the pixel milter).

```bash
# Send a test HTML email via sendmail
ssh smtp-uk "sendmail -v -t <<EOF
Subject: Test Email with Pixel
From: sender@example.com
To: recipient@example.com
MIME-Version: 1.0
Content-Type: text/html

<html>
<body>
<h1>Testing Pixel Injection</h1>
<p>This is a test email.</p>
</body>
</html>
EOF"
```

## 3. Deployment & Verification Workflow

The `deploy-to-smtp-uk.sh` script handles the deployment. If issues arise after deployment:

1.  **Verify Binaries**: Ensure the binaries on the remote server match the local build.
    ```bash
    ssh smtp-uk "ls -l /opt/pixel-tracking/"
    ```
2.  **Verify Configuration**: Check `master.cf` on the remote server to ensure macros are correctly expanded.
    ```bash
    ssh smtp-uk "cat /etc/postfix/master.cf | grep pixel"
    ```
    *Look for:* `flags=Rq user=nobody argv=/etc/postfix/pixel-content-filter.sh ${sender} ${recipient}`
3.  **Restart Services**: Sometimes a clean restart is needed.
    ```bash
    ssh smtp-uk "systemctl restart postfix pixelmilter pixelserver"
    ```

## 4. Common Errors & Solutions

*   **"Relay access denied"**:
    *   *Cause*: The client IP is not in `mynetworks` or SASL auth failed.
    *   *Check*: `postconf mynetworks` on the remote server.
*   **"Connection refused" on port 10025**:
    *   *Cause*: The reinjection listener in `master.cf` is not running or crashed.
    *   *Check*: `netstat -tulpn | grep 10025` and Postfix logs for "panic" or "fatal".
*   **Empty Sender/Recipient in Filter**:
    *   *Cause*: `master.cf` macros `${sender}` or `${recipient}` are not being passed correctly.
    *   *Fix*: Ensure `master.cf` defines the flags `flags=Rq` (or similar) and explicitly passes the arguments.

## 5. Network & Firewall Troubleshooting

Ensure the server is accessible and listening on the correct ports.

### Port Verification
Check which processes are listening on ports:
```bash
# Check listening ports (requires net-tools or iproute2)
ssh smtp-uk "netstat -tulpn"
# OR
ssh smtp-uk "ss -tulpn"
```

**Expected Ports:**
*   `25` (SMTP): Postfix (incoming mail)
*   `587` (Submission): Postfix (client submission)
*   `10025` (Reinjection): Postfix (internal use for content filter)
*   `8892` (Pixel Milter): `pixelmilter` (internal/localhost)
*   `8443` (Pixel Server): `pixelserver` (HTTPS for tracking pixel)

### Firewall Status
Check if the firewall is blocking connections.
```bash
# Check UFW status
ssh smtp-uk "ufw status verbose"

# Check IPTables rules
ssh smtp-uk "iptables -L -n -v"
```

## 6. SMTP Access Checks

Verify that the SMTP server is reachable and responding correctly from outside.

### External Connectivity Test
From your local machine (or another server), try to connect to the SMTP ports.

```bash
# Test SMTP (Port 25)
nc -zv smtp-uk.cloudzy.com 25

# Test Submission (Port 587)
nc -zv smtp-uk.cloudzy.com 587

# Manual SMTP Session (Telnet/Netcat)
nc smtp-uk.cloudzy.com 25
# Type: EHLO test.com
# Expected: 250-smtp-uk.cloudzy.com ...
```

### Open Relay Check
Ensure your server is NOT an open relay.
```bash
# Attempt to send mail from an external IP to an external address without auth
# This SHOULD FAIL with "Relay access denied"
telnet smtp-uk.cloudzy.com 25
EHLO external.com
MAIL FROM:<spammer@external.com>
RCPT TO:<victim@target.com>
# Expected Response: 554 5.7.1 <victim@target.com>: Relay access denied
```

## 7. Advanced Mail Queue Management

Managing the Postfix queue when things go wrong.

```bash
# List queue
ssh smtp-uk "mailq"

# Read a specific message in queue
ssh smtp-uk "postcat -q <QUEUE_ID>"

# Flush queue (attempt delivery now)
ssh smtp-uk "postqueue -f"

# Delete a specific message
ssh smtp-uk "postsuper -d <QUEUE_ID>"

# Delete ALL messages in queue (Use with caution!)
ssh smtp-uk "postsuper -d ALL"

# Hold a message (stop delivery attempts)
ssh smtp-uk "postsuper -h <QUEUE_ID>"

# Release a message from hold
ssh smtp-uk "postsuper -H <QUEUE_ID>"
```

## 8. DNS & Reputation Checks

Email delivery depends heavily on correct DNS records.

*   **Reverse DNS (PTR)**: The IP address must resolve back to the hostname (`smtp-uk.cloudzy.com`).
    ```bash
    host <SERVER_IP>
    ```
*   **SPF**: Check the TXT record for the sending domain.
    ```bash
    dig +short TXT yourdomain.com
    # Should include: v=spf1 mx ip4:<SERVER_IP> ~all
    ```
*   **DKIM**: Verify the public key is published.
    ```bash
    dig +short TXT default._domainkey.yourdomain.com
    ```
*   **DMARC**: Check DMARC policy.
    ```bash
    dig +short TXT _dmarc.yourdomain.com
    ```

