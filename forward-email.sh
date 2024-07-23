#!/bin/bash

# Read the destination email address from the revaliases file
destination=$(cat /etc/ssmtp/revaliases)

# Read email from stdin
email=$(cat)

# Forward email to the specified address
echo "$email" | mail -s "Forwarded Email" "$destination"
