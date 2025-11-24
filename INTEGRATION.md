# Integration Guide: Pixel Tracking for Postfix

This guide explains how to integrate the compiled `pixelmilter` and `pixelserver` binaries into an existing Postfix environment.

## Architecture Overview

The system consists of two main components that **must share a data directory**:

1.  **`pixelmilter`**: A Postfix Milter (Mail Filter) that intercepts outgoing emails.
    *   It modifies HTML emails to inject a tracking pixel (and optional footer).
    *   It creates metadata files in the shared data directory for each tracked email.
2.  **`pixelserver`**: A web server that hosts the tracking pixel.
    *   It serves the 1x1 transparent GIF.
    *   It updates the metadata files in the shared data directory when an email is opened.
    *   It provides an API/UI to view open statistics.

> **CRITICAL**: `pixelmilter` and `pixelserver` must have read/write access to the same `DATA_DIR` (e.g., `/var/lib/pixel-tracking`). If they run on different servers, you must use a shared filesystem (NFS, SMB, etc.).

---

## Prerequisites

1.  **Binaries**: Ensure you have the compiled binaries (`pixelmilter`, `pixelserver`) from the `bin/` directory.
2.  **SSL Certificates**: `pixelserver` requires valid SSL certificates (PEM format) to serve the pixel over HTTPS.
3.  **Shared Storage**: A directory writable by both services.

---

## Step 1: Deploy Pixel Server

The Pixel Server serves the tracking image. It should be publicly accessible via HTTPS.

1.  **Create Data Directory**:
    ```bash
    mkdir -p /var/lib/pixel-tracking
    chown -R nobody:nogroup /var/lib/pixel-tracking
    ```

2.  **Install Binary**:
    Copy `bin/pixelserver` to `/opt/pixel-tracking/`.

3.  **Create Systemd Service** (`/etc/systemd/system/pixelserver.service`):
    ```ini
    [Unit]
    Description=Pixel Tracking Server
    After=network.target

    [Service]
    Type=simple
    User=nobody
    Group=nogroup
    # Bind to 0.0.0.0:8443
    # Point to your SSL certs
    ExecStart=/opt/pixel-tracking/pixelserver \
        --data-dir /var/lib/pixel-tracking \
        --bind-address 0.0.0.0:8443 \
        --tls-cert /var/lib/pixel-tracking/ssl/cert.pem \
        --tls-key /var/lib/pixel-tracking/ssl/key.pem \
        --log-level info
    Restart=always

    [Install]
    WantedBy=multi-user.target
    ```

4.  **Start Service**:
    ```bash
    systemctl daemon-reload
    systemctl enable --now pixelserver
    ```

---

## Step 2: Deploy Pixel Milter

The Milter runs locally to Postfix (or accessible via network) and processes emails.

2.  **Install Binary**:
    Copy `bin/pixelmilter` to `/opt/pixel-tracking/`.

3.  **Create Systemd Service** (`/etc/systemd/system/pixelmilter.service`):
    ```ini
    [Unit]
    Description=Pixel Tracking Milter
    After=network.target

    [Service]
    Type=simple
    User=nobody
    Group=nogroup
    # PIXEL_BASE_URL must match your public pixelserver URL
    # Address can be a TCP port (0.0.0.0:8892) or Unix socket
    ExecStart=/opt/pixel-tracking/pixelmilter \
        --address 127.0.0.1:8892 \
        --data-dir /var/lib/pixel-tracking \
        --pixel-base-url "https://relay.example.com:8443/pixel?id=" \
        --tracking-requires-opt-in=false \
        --log-level info
    Restart=always

    [Install]
    WantedBy=multi-user.target
    ```

3.  **Start Service**:
    ```bash
    systemctl daemon-reload
    systemctl enable --now pixelmilter
    ```

---

## Step 3: Configure Postfix

You need to tell Postfix to send outgoing mail to the Milter.

1.  **Edit `main.cf`**:
    Add the Milter to your `smtpd_milters` or `non_smtpd_milters`.

    *   **For Outgoing Mail (Submission/587)**: This is the most common use case. You only want to track mail sent by your users.
        ```postfix
        # In /etc/postfix/master.cf, under the submission service:
        submission inet n       -       y       -       -       smtpd
          -o smtpd_milters=inet:localhost:8891,inet:127.0.0.1:8892
        ```

    *   **Global Configuration (main.cf)**:
        If you want to apply it globally (be careful not to track incoming spam):
        ```postfix
        # /etc/postfix/main.cf
        non_smtpd_milters = inet:127.0.0.1:8892
        milter_default_action = accept
        milter_protocol = 6
        ```

2.  **Reload Postfix**:
    ```bash
    systemctl reload postfix
    ```

---

## Step 4: Verification

1.  **Send an HTML Email**: Send an email from your server to an external address (e.g., Gmail).
2.  **Check Logs**:
    *   `journalctl -u pixelmilter -f`: Should show "Pixel injected successfully".
3.  **Check Email Source**:
    *   View the source of the received email.
    *   Look for `<img src="https://mail.yourdomain.com:8443/pixel?id=..." ...>`.
4.  **Verify Tracking**:
    *   Open the email (ensure images are loaded).
    *   Check `journalctl -u pixelserver -f`: Should show "Tracking data updated successfully".
    *   Check the reports UI at `https://mail.yourdomain.com:8443/` (if configured).

---

## Advanced: Content Filter Mode (Alternative)

If you cannot use the Milter protocol, `pixelmilter` can run as a content filter (stdin -> stdout).

1.  **Command**:
    ```bash
    /opt/pixel-tracking/pixelmilter --content-filter-mode --data-dir ...
    ```
2.  **Postfix Integration**:
    Requires setting up a `pipe` transport in `master.cf` that pipes to a script running the above command, and then uses `postfix-reinject` (also provided in `bin/`) to feed the result back into Postfix on a different port (e.g., 10025).


## View logs

```
#!/bin/bash
sudo journalctl -u postfix -u pixelmilter -u pixelserver -f "$@"
```
