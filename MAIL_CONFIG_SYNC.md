# Mail Configuration Sync Mechanism

## Overview

The admin panel automatically synchronizes email configuration from the SQLite database to Postfix and Dovecot. This document explains how the sync mechanism works.

## Architecture

```
Admin Panel (SQLite) → Job Queue → Config Files → Postfix/Dovecot
```

### Components

1. **Admin Panel**: Laravel application with SQLite database
2. **Job Queue**: Database-backed queue for async processing
3. **Shared Directory**: `./data/mail-config` mounted in all containers
4. **Config Files**: Traditional Postfix/Dovecot config format

## How It Works

### 1. Model Events

When domains, email accounts, or aliases are created, updated, or deleted:

```php
// In Domain, EmailAccount, and Alias models
protected static function booted(): void
{
    static::saved(function () {
        SyncMailConfigJob::dispatch()->onQueue('mail-sync');
    });
    
    static::deleted(function () {
        SyncMailConfigJob::dispatch()->onQueue('mail-sync');
    });
}
```

### 2. Job Queue Processing

The `SyncMailConfigJob` runs in the background with:
- **Cache Lock**: Prevents overlapping sync processes (10-second lock)
- **Debouncing**: Multiple rapid changes batch into single sync
- **Error Handling**: Logs failures without breaking the app

### 3. Config File Generation

The job generates these files in `storage/app/mail-config/`:

```
virtual_domains      - List of active domains
vmailbox             - Email to mailbox mappings
virtual_aliases      - Email alias forwarding rules
dovecot_passwd       - User authentication (bcrypt with {BLF-CRYPT} prefix)
```

### 4. File Mounting

The shared directory is mounted into containers:

```yaml
# Admin container
volumes:
  - ./data/mail-config:/var/www/html/storage/app/mail-config:rw

# Postfix container
volumes:
  - ./data/mail-config:/var/mailserver/config:ro

# Dovecot container
volumes:
  - ./data/mail-config:/var/mailserver/config:ro
```

### 5. Postfix/Dovecot Integration

Postfix and Dovecot need to be configured to read from these files:

**Postfix main.cf:**
```
virtual_mailbox_domains = hash:/var/mailserver/config/virtual_domains
virtual_mailbox_maps = hash:/var/mailserver/config/vmailbox
virtual_alias_maps = hash:/var/mailserver/config/virtual_aliases
```

**Dovecot passdb:**
```
passdb {
  driver = passwd-file
  args = scheme=BLF-CRYPT username_format=%u /var/mailserver/config/dovecot_passwd
}
```

## File Formats

### virtual_domains
```
example.com
another.com
```

### vmailbox
```
user@example.com    user/
admin@another.com   admin/
```

### virtual_aliases
```
alias@example.com       user@example.com
support@another.com     admin@another.com
```

### dovecot_passwd
```
user@example.com:{BLF-CRYPT}$2y$10$...
admin@another.com:{BLF-CRYPT}$2y$10$...
```

## Atomic Writes

Files are written atomically to prevent partial updates:

1. Write to temporary file (e.g., `virtual_domains.tmp`)
2. Set permissions
3. Rename to final name (atomic operation)

This ensures Postfix/Dovecot never read incomplete files.

## Cache Lock

The sync uses a 10-second cache lock:

```php
$lock = Cache::lock('mail-config-sync', 10);

if (!$lock->get()) {
    // Another sync is running, skip this one
    return;
}
```

Benefits:
- Prevents race conditions
- Handles rapid successive changes
- Natural debouncing via job queue

## Queue Worker

The admin container runs a queue worker via Supervisor:

```bash
php artisan queue:work --queue=mail-sync --tries=3 --timeout=90
```

This processes sync jobs in the background without blocking web requests.

## Troubleshooting

### Config files not updating

1. Check queue worker is running:
   ```bash
   docker exec mailserver_admin ps aux | grep queue
   ```

2. Check logs:
   ```bash
   docker logs mailserver_admin | grep -i sync
   ```

3. Check failed jobs:
   ```bash
   docker exec mailserver_admin php artisan queue:failed
   ```

### Permission errors

Ensure the shared directory is writable by the admin container:
```bash
chmod 755 ./data/mail-config
```

### Lock timeouts

If syncs are taking longer than 10 seconds, increase the lock timeout in `SyncMailConfigJob.php`:
```php
$lock = Cache::lock('mail-config-sync', 30); // Increase to 30 seconds
```

## Performance

- **Sync Time**: < 1 second for typical configuration
- **Lock Duration**: 10 seconds maximum
- **Queue Processing**: Near real-time (< 5 seconds)
- **Impact**: Zero impact on admin panel performance

## Security

- Config files mounted read-only in Postfix/Dovecot
- Passwords hashed with bcrypt
- Dovecot scheme prefix prevents plaintext interpretation
- Atomic writes prevent race conditions
- Cache lock prevents concurrent modifications
