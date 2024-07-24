#!/bin/bash
# save_email.sh

# Generate a unique filename based on the current timestamp
FILENAME="/var/mail/$(date +%s%N).eml"

# Save the email to the file
cat > "$FILENAME"
cat "$FILENAME" >> /var/mail/vmail/inbox

# Set file permissions to readonly
chown vmail:vmail "$FILENAME"
chmod 644 "$FILENAME"
