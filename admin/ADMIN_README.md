# Mail Server Administration Panel

A Laravel-based web administration panel for managing email domains, accounts, and aliases.

## Quick Start

1. Start services: `docker-compose up -d db admin webmail`
2. Run migrations: `docker-compose exec admin php artisan migrate`
3. Create admin: `docker-compose exec admin php artisan tinker` then create AdminUser
4. Access admin at: http://your-server:8080
5. Access webmail at: http://your-server:8081

See full documentation in admin/README.md
