# Use an official Ubuntu as a parent image
FROM ubuntu:latest

# Install ssmtp and mailutils
RUN apt-get update && apt-get install -y ssmtp mailutils

# Copy configuration files
COPY ssmtp.conf /etc/ssmtp/ssmtp.conf
COPY revaliases /etc/ssmtp/revaliases
COPY forward-email.sh /usr/local/bin/forward-email.sh

# Make the script executable
RUN chmod +x /usr/local/bin/forward-email.sh

# Entry point
CMD ["ssmtp", "-t"]
