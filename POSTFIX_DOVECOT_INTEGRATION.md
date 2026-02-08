# Postfix/Dovecot Integration with Admin Panel

This document describes how the Postfix and Dovecot mail services integrate with the Laravel admin panel's SQLite database.

## Architecture

The admin panel stores all configuration in a SQLite database at `data/admin/database.sqlite`. This database is mounted read-only into both Postfix and Dovecot containers.

## Data Flow

1. **Admin Panel** → SQLite Database (read/write)
2. **SQLite Database** → Postfix & Dovecot (read-only mount)
3. **Sync Script** → Exports SQLite data to traditional config files

## Database Tables

### domains
- Stores email domains configured in the system
- Fields: domain, active, description, dkim_*

### email_accounts
- Stores email user accounts
- Fields: email, username, password (bcrypt), quota, active
- Used by Dovecot for authentication

### aliases
- Stores email forwarding rules
- Fields: source, destination, active
- Used by Postfix for aliasing

## Integration Method

The integration uses a sync script that runs periodically to export SQLite data into traditional Postfix/Dovecot configuration files:

### For Postfix:
- `/etc/postfix/virtual_domains` - List of virtual domains
- `/etc/postfix/vmailbox` - Virtual mailbox mappings
- `/etc/postfix/virtual_aliases` - Virtual alias mappings

### For Dovecot:
- `/etc/dovecot/passwd` - User authentication file (email:password_hash format)

## Sync Script

The `admin/sync-config.sh` script:
1. Reads from the SQLite database
2. Exports data to text files
3. Reloads Postfix and Dovecot services

This script should be run:
- After any changes in the admin panel
- Via cron job every minute for automatic sync
- Manually when needed

## Setup Instructions

### 1. Enable Auto-Sync (Recommended)

Add a cron job to the admin container:

```bash
# Add to admin container's crontab
* * * * * /var/www/html/sync-config.sh >> /var/log/sync.log 2>&1
```

### 2. Manual Sync

Run the sync script manually:

```bash
docker-compose exec admin /var/www/html/sync-config.sh
```

### 3. Verify Integration

Check that config files are generated:

```bash
# Check Postfix configs
docker-compose exec postfix cat /etc/postfix/virtual_domains
docker-compose exec postfix cat /etc/postfix/vmailbox
docker-compose exec postfix cat /etc/postfix/virtual_aliases

# Check Dovecot config
docker-compose exec dovecot cat /etc/dovecot/passwd
```

## Password Format

The admin panel stores passwords using bcrypt hashing (Laravel's default). The sync script exports these hashes in the format required by Dovecot:

```
email:bcrypt_hash
```

Dovecot must be configured to use bcrypt for password verification.

## Data Persistence

The SQLite database is persisted in `data/admin/database.sqlite` on the host. This ensures:
- No data loss between container restarts
- Easy backups (just copy the file)
- Portable configuration

## Network Security

The admin panel and database are only accessible within the Docker network (`mailnet`) except:
- Admin panel: Port 8080 (HTTP interface for administrators)
- Postfix: Ports 25, 587, 465 (SMTP)
- Dovecot: Ports 143, 993 (IMAP)
- PixelServer: Ports 8443, 8444 (HTTPS analytics & reports)

All internal communication happens via the internal Docker network.

## Future Enhancements

Potential improvements:
- Direct SQLite integration (Postfix/Dovecot reading from SQLite)
- Real-time sync using file watchers
- API endpoint for programmatic sync triggering
- Webhook notifications on configuration changes
