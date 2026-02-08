# Mail Server Admin Panel

Simple Laravel-based administration interface for managing email domains, accounts, and aliases.

## Features

- ✨ **No JavaScript** - Pure HTML, works everywhere
- 🔓 **No Authentication** - Direct access for simplicity
- 💾 **SQLite Database** - Lightweight file-based storage
- 🎨 **Clean UI** - Responsive design with inline CSS

## Structure

- `app/` - Controllers and Models
- `database/` - Migrations and SQLite database file
- `resources/views/` - Blade templates (NO JavaScript)
- `routes/web.php` - Application routes

## Database Schema

### Domains
- `domain` - Domain name (e.g., example.com)
- `description` - Optional description
- `active` - Status flag
- DKIM fields for email signing

### Email Accounts
- `email` - Full email address
- `username` - Local part
- `password` - Hashed password
- `quota` - Storage limit in bytes (0 = unlimited)
- `active` - Status flag

### Aliases
- `source` - Source email or pattern
- `destination` - Destination email
- `active` - Status flag

## Development

This is a standard Laravel 12 application running in production mode with:
- SQLite database (no MySQL needed)
- Session-based routing
- Server-side rendering only
- No API endpoints
- No authentication system

## Technology Stack

- **Laravel 12** - PHP framework
- **PHP 8.3** - Server-side language
- **SQLite** - Database
- **Blade** - Templating engine
- **Nginx** - Web server
- **PHP-FPM** - FastCGI process manager
