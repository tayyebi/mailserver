# GitHub Copilot Instructions - Mailserver Project

This document provides context and technical details about the mailserver architecture, specifically focusing on the internal workings of Postfix and Dovecot as configured in this project. Reference this file when debugging issues, refactoring configuration logic, or explaining system behavior.

## 1. Project Overview

This is a Rust-based mailserver solution that generates configuration for a standard Linux mail stack:
- **MTA**: Postfix
- **MDA/POP3/IMAP**: Dovecot
- **Spam Filtering**: Rspamd (implied integration via milters/pipes)
- **Database**: PostgreSQL (user management, aliases, domains)
- **Web Interface**: Rust (Actix-web) application for management

The configuration logic is centrally managed in `src/config.rs`, which generates the necessary flat files and database maps for the underlying services.

## 2. Postfix Architecture & "Under the Hood"

Postfix is a modular system of small, single-purpose executable programs orchestrated by a master daemon.

### Key Processes (Daemons)
- **`master` (PID 1 of mail system)**: The supervisor. It reads `master.cf` and spawns all other daemons (smtpd, qmgr, pickup, cleanup) on demand. It enforces process limits and health checks.
- **`smtpd`**: Handles incoming SMTP connections (port 25, 465, 587). Performs initial policy checks (HELO, sender validation) and SASL authentication via Dovecot.
- **`pickup`**: Retrieves locally submitted mail (from `sendmail` command or `maildrop` queue) and feeds it into the system. It's the entry point for locally generated mail.
- **`cleanup`**: The gatekeeper. It processes all incoming mail (from `smtpd` or `pickup`). This is where **address rewriting** (canonical maps, virtual aliases) and header normalization happens before the mail is queued.
- **`qmgr`**: The central scheduler. It scans the *incoming* and *deferred* queues, moves actionable mail to *active*, and allocates delivery agents (smtp, local, pipe, virtual) to deliver the mail.

### Configuration Files
- **`main.cf`**: Global parameters. Configuration logic in `src/config.rs` generates this.
- **`master.cf`**: Service definitions. Defines *how* daemons run (chroot, privileges, command arguments).
  - **Important**: Options in `master.cf` can override `main.cf` parameters using `-o parameter=value`.
  - **Recent Fix**: We explicitly configured `pixelfilter` and `pixelfilter-in` services here.

### The `pipe` Delivery Agent
The `pipe` daemon allows Postfix to hand off delivery to external programs (like our webhook filter).
- **Architecture**: It is a long-running daemon that forks/execs the specified command.
- **Privilege Requirements**:
  - The `pipe` daemon **cannot** execute external commands as root.
  - In `master.cf`, the `user=` attribute is **mandatory** for any `pipe` service.
  - The `flags` attribute controls extensive behavior (e.g., `DRhu` prepends `Delivered-To` headers, adds `Requires` like `From` lines).
- **Common Error**: "service ... requires privileged operation" usually means the `pipe` service was defined as unprivileged (`y`) in the `unpriv` column of `master.cf`, but the command requires setuid or similar higher-level system access constraints that the restricted `pipe` environment doesn't satisfy effectively without specific `user=` directives being handled correctly by the master daemon. In our case, we set `unpriv` to `n` (no, not unprivileged = privileged) to ensure correct execution context.

### Lookup Tables & Maps
Postfix decouples configuration data into lookup tables.
- **`virtual_alias_maps`**: **Address Rewriting**. Maps `user@domain` -> `user@domain` (identity) or `alias@domain` -> `destination`. 
  - **Specifics**: Recursive resolution occurs here.
  - **Logic**: Our `build_virtual_alias_entries` function ensures catch-alls (`@domain`) don't override specific accounts by injecting identity entries.
- **`virtual_mailbox_maps`**: **Validation & Location**. Tells Postfix "this user exists" and usually provides the path/instruction for delivery.
- **`transport_maps`**: **Routing**. Overrides default routing. Used for outbound relays (`example.com smtp:[relay.provider.com]`).
- **`smtpd_sender_login_maps`**: **Anti-Spoofing**. Maps `Envelope Sender` -> `SASL Username`. Used to prevent authenticated users from sending mail as other users.

### Message Flow & Rewriting Order
1. **Input**: `smtpd` receives mail.
2. **Cleanup**: 
   - `canonical_maps` (address rewriting).
   - `virtual_alias_maps` (aliasing/forwarding). **Crucial**: This happens *before* routing.
3. **Queue**: Message sits in `incoming` -> `active`.
4. **Routing (`qmgr`)**: Checks `transport_maps`, then address classes (local, virtual, relay).
5. **Delivery**: Passes distinct recipients to delivery agents (`smtp`, `pipe`, `virtual` (dovecot-lmtp)).

## 3. Dovecot Architecture & "Under the Hood"

Dovecot acts as the MDA (Mail Delivery Agent) and provides POP3/IMAP access.

### Key Processes
- **`dovecot` (Master)**: Manages all other processes. Connects to `auth` and `config` sockets.
- **`auth`**: Handles authentication database lookups (**PassDB** and **UserDB**). It verifies passwords and looks up user home directories/UIDs.
- **`lmtp`**: Local Mail Transfer Protocol. Receives mail from Postfix for final local delivery. It's preferred over `dovecot-lda` because it's a long-running daemon (higher performance) and can reject mail during the SMTP transaction (e.g., over quota).
- **`imap`/`pop3`**: Per-user processes handling client viewing/downloading of mail.

### Mail Storage
- **Format**: **Maildir** (one file per message).
  - Directory structure: `cur` (seen), `new` (unseen), `tmp` (delivery in progress).
  - Configured Path: `/var/mail/vhosts/%d/%n` (domain/username).
  - **Pros**: Robust, no locking contentions, resilient to crashes.
  - **Cons**: Can be slow with massive folders (tens of thousands of messages) on some filesystems (but acceptable for this scale).

### Authentication Integration
Dovecot creates a listening UNIX socket (usually `/var/spool/postfix/private/auth`) that Postfix connects to.
- **Mechanism**: SASL (Simple Authentication and Security Layer).
- **Flow**: 
  1. Client connects to Postfix `smtpd`.
  2. Postfix talks to Dovecot `auth` socket.
  3. Dovecot checks `passwd-file` or SQL backend.
  4. Dovecot returns Success/Failure to Postfix.

### SQL Database Integration
In this project, user data resides in PostgreSQL but config generation currently creates flat files for some services to ensure performance and reliability even if the DB is momentarily unreachable (or for simplicity).
- **Password Scheme**: `{BLF-CRYPT}` (Blowfish/bcrypt) is the standard for secure password storage in Dovecot.

## 4. OpenSSL & Security
- **Certificates**: Generated via `openssl` command-line tools in `src/config.rs`.
- **Permissions**: **Critical**. Private keys must be read-only by root (or the specific service user) and effectively 0600.
- **Diffie-Hellman (DH) Params**: Generated to ensure unrelated sessions cannot be decrypted even if the private key is compromised later (Forward Secrecy).

## 5. Specific Refactoring Notes (Feb 2026)
- **Strings**: We strictly avoid `\n` concatenation in Rust code for config generation. Use `writeln!` macro or raw string literals (`r#""#`) for readability and reliability.
- **Templates**: Logic is moving towards external template files (`templates/config/*.txt`) to decouple Rust code from configuration syntax.
- **Formatting**: All config generation functions use `generated_header()` to mark files as auto-generated.

## How to use this knowledge
- When user asks about "why is email rejected?", check the **Cleanup** and **Routing** phases.
- When configuring new Postfix modules, remember to check `master.cf` privileges.
- When debugging "user unknown", verify `virtual_mailbox_maps` vs `virtual_alias_maps` contents.
