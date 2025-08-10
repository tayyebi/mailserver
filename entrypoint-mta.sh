#!/bin/bash

# Initialize Postfix virtual domains
echo "Configuring virtual domains: $DOMAINS"

# Configure Postfix to accept ANY domain
postconf -e "virtual_alias_domains = /etc/postfix/virtual_domains"
postconf -e "relay_domains = /etc/postfix/virtual_domains"

# Create virtual aliases file
cat <<EOF > /etc/postfix/virtual
# Virtual aliases for all domains
postmaster@tyyi.net    info@gordarg.com
postmaster@0pt.ir      info@gordarg.com
abuse@tyyi.net         info@gordarg.com
abuse@0pt.ir           info@gordarg.com


# Domain catch-all (for your hosted domains)
# @tyyi.net              info@gordarg.com

# Global catch-all (for ANY domain - use cautiously!)
/.*/                   info@gordarg.com

EOF

# Generate virtual.db
echo "Generating virtual database"
echo chown root:postfix /etc/postfix/virtual && \
     chmod 640 /etc/postfix/virtual && \
     postmap -v /etc/postfix/virtual

# Enable recipient catch-all
postconf -e "virtual_alias_maps = hash:/etc/postfix/virtual"
postconf -e "luser_relay = info@gordarg.com"
postconf -e "local_recipient_maps = "
postconf -e "relay_recipient_maps = "

# Set permissions for OpenDKIM
chown -R opendkim:opendkim /etc/opendkim
chmod 640 /etc/opendkim/keys/*/*.private

# Prevent becoming an open relay
postconf -e "smtpd_relay_restrictions = permit_mynetworks, reject_unauth_destination"

# Rate limiting
postconf -e "smtpd_client_message_rate_limit = 100"
postconf -e "anvil_rate_time_unit = 60s"


# SMTPs/Submission explicit service definitions
postconf -M -e "submission/inet=submission inet n - n - - smtpd"
postconf -P -e "submission/inet/syslog_name=postfix/submission"
postconf -P -e "submission/inet/smtpd_tls_security_level=encrypt"
postconf -P -e "submission/inet/smtpd_sasl_auth_enable=yes"
postconf -P -e "submission/inet/milter_macro_daemon_name=ORIGINATING"

postconf -M -e "smtps/inet=smtps inet n - n - - smtpd"
postconf -P -e "smtps/inet/syslog_name=postfix/smtps"
postconf -P -e "smtps/inet/smtpd_tls_wrappermode=yes"
postconf -P -e "smtps/inet/smtpd_sasl_auth_enable=yes"
postconf -P -e "smtps/inet/milter_macro_daemon_name=ORIGINATING"

# Start services
# rsyslogd &
# opendkim -f -x /etc/opendkim.conf &

echo "Starting Postfix"
exec postfix start-fg -D -v
