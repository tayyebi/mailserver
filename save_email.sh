#!/bin/bash
# save_email.sh

# Create a directory to store emails if it doesn't exist
mkdir -p /var/mail/

# Generate a unique filename based on the current timestamp
FILENAME="/var/mail/$(date +%s%N).eml"

# Save the email to the file
cat > "$FILENAME"