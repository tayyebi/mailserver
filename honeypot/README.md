# Honeypot

A simple Rust-based honeypot that listens on specified ports and simulates various services (SMTP, HTTP, SSH) to trap and log unauthorized connections.

## Configuration

The honeypot is configured via a JSON file (default: `honeypot.json`).

```json
{
  "whitelist": [
    "127.0.0.1",
    "::1"
  ],
  "services": {
    "2222": "ssh",
    "8080": "http",
    "2525": "smtp"
  }
}
```

- `whitelist`: List of IP addresses that are allowed to connect (ignored by the honeypot).
- `services`: Map of Port -> Service Type. Supported types: `smtp`, `http`, `ssh`, `generic`.

## Building

The honeypot is included in the project's `make build-binaries` command.

To build manually:
```bash
cd honeypot
cargo build --release
```

## Running

```bash
./target/release/honeypot [config_file]
```

If `config_file` is not provided, it looks for `honeypot.json` in the current directory.

## Logging

The honeypot uses `env_logger`. You can control the log level via the `RUST_LOG` environment variable.

```bash
RUST_LOG=info ./honeypot
```

Logs will show trapped connections and the data they sent.
