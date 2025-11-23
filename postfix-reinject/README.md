# Postfix Reinjection Tool (Rust)

This tool reads an email message from stdin, extracts the sender and recipient, and reinjects the message to a local SMTP service (127.0.0.1:10025) for further delivery by Postfix.

## Build

```sh
cd postfix-reinject
cargo build --release
```

The binary will be at `target/release/postfix-reinject`.

## Usage

In your Postfix content filter setup, use this tool as the reinjection command:

```
content_filter = ...
...
filter_destination_recipient_limit = 1
...

# In master.cf:
reinject   unix  -       n       n       -       -       pipe
  flags=Rq user=postfix argv=/path/to/postfix-reinject
```

## Notes
- No TLS is used for reinjection (port 10025).
- If the `From` header is missing, defaults to `noreply@localhost`.
- If the `To` header is missing, the message is rejected.

## License
MIT
