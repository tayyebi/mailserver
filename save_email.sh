#!/bin/bash
# save_email.sh

# Generate a unique filename based on the current timestamp
FILENAME="/var/mail/$(date +%s%N).eml"

# Save the email to the file
cat > "$FILENAME"
cat "$FILENAME" >> /var/mail/root/inbox

# Set file permissions to readonly
chown root:root "$FILENAME"
chmod 644 "$FILENAME"
