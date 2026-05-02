//! Auto-provisioning: connect to a remote server via SSH, install all mailserver
//! dependencies, upload the current binary and supporting files, and configure
//! the system service — idempotently (already-done steps are skipped).
//!
//! Credentials are only held in memory for the duration of the SSH session and
//! are never written to disk.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use log::{error, info, warn};
use russh::client;
use russh::client::AuthResult;
use russh::keys::{PrivateKey, PrivateKeyWithHashAlg};
use russh::ChannelMsg;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ── SSH Handler ───────────────────────────────────────────────────────────────

/// Minimal russh client handler. Accepts all host keys (users are expected to
/// verify the host key fingerprint printed to the log before trusting the remote).
struct SshHandler;

impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        info!(
            "[provision] remote host key algorithm: {}",
            server_public_key.algorithm()
        );
        info!("[provision] accepting host key — verify algorithm/fingerprint above if this is your first connection");
        Ok(true)
    }
}

// ── Param Parsing ─────────────────────────────────────────────────────────────

struct Params {
    host: String,
    port: u16,
    user: String,
    key_path: Option<PathBuf>,
    password: Option<String>,
    /// Optional path to a env-file that will be uploaded to /etc/mailserver/env
    env_file: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<Params, String> {
    let mut host = None;
    let mut port: u16 = 22;
    let mut user = None;
    let mut key_path = None;
    let mut password = None;
    let mut env_file = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--host" => {
                i += 1;
                host = Some(args.get(i).ok_or("--host requires a value")?.clone());
            }
            "--port" => {
                i += 1;
                port = args
                    .get(i)
                    .ok_or("--port requires a value")?
                    .parse::<u16>()
                    .map_err(|_| "--port must be a valid port number (1-65535)")?;
            }
            "--user" | "--username" => {
                i += 1;
                user = Some(args.get(i).ok_or("--user requires a value")?.clone());
            }
            "--key" => {
                i += 1;
                let v = args.get(i).ok_or("--key requires a path")?;
                key_path = Some(PathBuf::from(v));
            }
            "--password" => {
                i += 1;
                password = Some(args.get(i).ok_or("--password requires a value")?.clone());
            }
            "--env-file" => {
                i += 1;
                let v = args.get(i).ok_or("--env-file requires a path")?;
                env_file = Some(PathBuf::from(v));
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
        i += 1;
    }

    Ok(Params {
        host: host.ok_or("--host is required")?,
        port,
        user: user.ok_or("--user is required")?,
        key_path,
        password,
        env_file,
    })
}

// ── Entry Point ───────────────────────────────────────────────────────────────

/// Run the `provision` command.  `args` is the slice of CLI arguments that
/// follow the `provision` subcommand token.
pub async fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let params = match parse_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("provision: {}", e);
            eprintln!();
            print_usage();
            std::process::exit(1);
        }
    };

    info!(
        "[provision] connecting to {}:{} as user '{}'",
        params.host, params.port, params.user
    );

    let mut session = connect_ssh(&params).await?;

    info!("[provision] SSH session established. Starting provisioning sequence.");
    info!("[provision] ─── step 1/8: detect package manager ───");
    let pkg_mgr = detect_pkg_manager(&mut session).await?;
    info!("[provision] package manager: {}", pkg_mgr);

    info!("[provision] ─── step 2/8: install system dependencies ───");
    install_deps(&mut session, &pkg_mgr).await?;

    info!("[provision] ─── step 3/8: create users and directories ───");
    setup_users_and_dirs(&mut session, &pkg_mgr).await?;

    info!("[provision] ─── step 4/8: upload mailserver binary ───");
    upload_binary(&mut session).await?;

    info!("[provision] ─── step 5/8: upload supporting files ───");
    upload_support_files(&mut session).await?;

    if let Some(ref ef) = params.env_file {
        info!("[provision] uploading env file: {:?}", ef);
        upload_file(
            &mut session,
            ef.to_str()
                .ok_or("env file path contains invalid UTF-8")?,
            "/etc/mailserver/env",
            false,
            true,
        )
        .await?;
        info!("[provision] env file uploaded to /etc/mailserver/env");
    } else {
        info!("[provision] --env-file not specified; skipping env file upload");
        info!("[provision] hint: create /etc/mailserver/env on the remote with DATABASE_URL etc.");
    }

    info!("[provision] ─── step 6/8: initial mailserver setup ───");
    initial_setup(&mut session).await?;

    info!("[provision] ─── step 7/8: configure system service ───");
    setup_service(&mut session, &pkg_mgr).await?;

    info!("[provision] ─── step 8/8: enable and start service ───");
    start_service(&mut session, &pkg_mgr).await?;

    info!("[provision] ─────────────────────────────────────────────");
    info!("[provision] provisioning complete!");

    session
        .disconnect(russh::Disconnect::ByApplication, "done", "English")
        .await?;

    Ok(())
}

// ── SSH Connection ────────────────────────────────────────────────────────────

async fn connect_ssh(
    params: &Params,
) -> Result<client::Handle<SshHandler>, Box<dyn std::error::Error>> {
    let config = Arc::new(client::Config::default());
    let addr = (params.host.as_str(), params.port);

    let mut session = client::connect(config, addr, SshHandler).await?;

    // Try public-key authentication first
    let mut authed = false;
    if let Some(ref key_path) = params.key_path {
        info!("[provision] attempting public-key authentication with {:?}", key_path);
        match load_key(key_path, params.password.as_deref()) {
            Ok(key) => {
                let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key), None);
                match session.authenticate_publickey(&params.user, key_with_alg).await {
                    Ok(AuthResult::Success) => {
                        info!("[provision] public-key authentication succeeded");
                        authed = true;
                    }
                    Ok(AuthResult::Failure { .. }) => {
                        warn!("[provision] public-key authentication rejected by server");
                    }
                    Err(e) => {
                        warn!("[provision] public-key authentication error: {}", e);
                    }
                }
            }
            Err(e) => {
                warn!("[provision] failed to load private key {:?}: {}", key_path, e);
            }
        }
    }

    // Fall back to password authentication
    if !authed {
        if let Some(ref pwd) = params.password {
            info!("[provision] attempting password authentication");
            match session.authenticate_password(&params.user, pwd).await {
                Ok(AuthResult::Success) => {
                    info!("[provision] password authentication succeeded");
                    authed = true;
                }
                Ok(AuthResult::Failure { .. }) => {
                    warn!("[provision] password authentication rejected by server");
                }
                Err(e) => {
                    return Err(format!("password authentication error: {}", e).into());
                }
            }
        }
    }

    if !authed {
        return Err("all authentication methods failed; check --key / --password".into());
    }

    Ok(session)
}

/// Load a private key from disk, trying without a passphrase first and then
/// with the supplied password as passphrase.
fn load_key(path: &Path, password: Option<&str>) -> Result<PrivateKey, Box<dyn std::error::Error>> {
    // Try passphrase-protected first if a password was given
    if let Some(pwd) = password {
        if let Ok(kp) = russh::keys::load_secret_key(path, Some(pwd)) {
            return Ok(kp);
        }
    }
    // Try unencrypted key
    let kp = russh::keys::load_secret_key(path, None)?;
    Ok(kp)
}

// ── Remote Execution Helpers ──────────────────────────────────────────────────

/// Result of a remote command: (stdout, stderr, exit_code)
struct CmdResult {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

impl CmdResult {
    fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Execute a single command on the remote host and collect stdout/stderr.
async fn exec(
    session: &mut client::Handle<SshHandler>,
    cmd: &str,
) -> Result<CmdResult, Box<dyn std::error::Error>> {
    let mut channel = session.channel_open_session().await?;
    channel.exec(true, cmd).await?;

    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code: i32 = -1;

    loop {
        match channel.wait().await {
            Some(ChannelMsg::Data { data }) => {
                stdout.push_str(&String::from_utf8_lossy(&data));
            }
            Some(ChannelMsg::ExtendedData { data, .. }) => {
                stderr.push_str(&String::from_utf8_lossy(&data));
            }
            Some(ChannelMsg::ExitStatus { exit_status }) => {
                exit_code = exit_status as i32;
            }
            None => break,
            _ => {}
        }
    }

    Ok(CmdResult {
        stdout,
        stderr,
        exit_code,
    })
}

/// Execute a command and log stdout/stderr at debug/warn level.
/// Returns the exit code.
async fn run_remote(
    session: &mut client::Handle<SshHandler>,
    description: &str,
    cmd: &str,
) -> Result<i32, Box<dyn std::error::Error>> {
    info!("[provision] $ {}", cmd);
    let res = exec(session, cmd).await?;

    let out = res.stdout.trim().to_string();
    let err = res.stderr.trim().to_string();

    if !out.is_empty() {
        for line in out.lines() {
            info!("[provision]   {}", line);
        }
    }
    if !err.is_empty() {
        for line in err.lines() {
            warn!("[provision]   stderr: {}", line);
        }
    }

    if res.success() {
        info!("[provision] ✓ {}", description);
    } else {
        warn!(
            "[provision] ✗ {} (exit code {})",
            description, res.exit_code
        );
    }

    Ok(res.exit_code)
}

/// Check whether a remote file or directory exists.
async fn remote_exists(
    session: &mut client::Handle<SshHandler>,
    path: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let res = exec(session, &format!("test -e {}", path)).await?;
    Ok(res.success())
}

/// Check whether a remote command is available in PATH.
async fn command_exists(
    session: &mut client::Handle<SshHandler>,
    command: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let res = exec(session, &format!("command -v {} >/dev/null 2>&1", command)).await?;
    Ok(res.success())
}

// ── File Upload ───────────────────────────────────────────────────────────────

/// Upload a local file to the remote host via the SSH exec channel.
/// The file is base64-encoded on the local side and decoded on the remote side,
/// so no out-of-band protocol (SCP/SFTP) is required.
///
/// For large files (> 60 KB) the content is split into multiple `dd` append
/// blocks to stay within shell argument length limits.
///
/// * `skip_if_exists` — when `true` the upload is skipped if the remote file
///   already exists.
async fn upload_file(
    session: &mut client::Handle<SshHandler>,
    local_path: &str,
    remote_path: &str,
    executable: bool,
    skip_if_exists: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_exists && remote_exists(session, remote_path).await? {
        info!("[provision] skip upload (already exists): {}", remote_path);
        return Ok(());
    }

    info!("[provision] uploading {} → {}", local_path, remote_path);
    let data = std::fs::read(local_path)
        .map_err(|e| format!("cannot read local file {}: {}", local_path, e))?;

    // Ensure parent directory exists
    if let Some(parent) = Path::new(remote_path).parent() {
        let parent_str = parent.to_string_lossy();
        if !parent_str.is_empty() && parent_str != "/" {
            exec(session, &format!("mkdir -p {}", parent_str)).await?;
        }
    }

    // Split into 48 KiB chunks (base64 of 48 KiB ≈ 65 KiB, well within Linux
    // ARG_MAX and most SSH server limits).
    const CHUNK: usize = 48 * 1024;
    let total = data.len();
    let chunks: Vec<&[u8]> = data.chunks(CHUNK).collect();
    let n = chunks.len();

    info!("[provision] file size: {} bytes, {} chunk(s)", total, n);

    // Truncate (or create) the remote file first
    exec(session, &format!("> {}", remote_path)).await?;

    for (idx, chunk) in chunks.iter().enumerate() {
        let encoded = BASE64.encode(chunk);
        // Use printf instead of echo to avoid interpretation of escape sequences
        let cmd = format!(
            "printf '%s' '{}' | base64 -d >> {}",
            encoded, remote_path
        );
        let res = exec(session, &cmd).await?;
        if !res.success() {
            return Err(format!(
                "upload chunk {}/{} to {} failed (exit {})",
                idx + 1,
                n,
                remote_path,
                res.exit_code
            )
            .into());
        }
    }

    if executable {
        exec(session, &format!("chmod +x {}", remote_path)).await?;
    }

    info!("[provision] ✓ uploaded {}", remote_path);
    Ok(())
}

/// Upload all `*.txt` template files under a local directory tree to the
/// remote, preserving relative paths under `remote_base`.
async fn upload_dir(
    session: &mut client::Handle<SshHandler>,
    local_dir: &str,
    remote_base: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let local_path = Path::new(local_dir);
    if !local_path.exists() {
        info!("[provision] local directory {} does not exist, skipping", local_dir);
        return Ok(());
    }

    exec(session, &format!("mkdir -p {}", remote_base)).await?;

    upload_dir_recursive(session, local_path, local_path, remote_base).await
}

fn upload_dir_recursive<'a>(
    session: &'a mut client::Handle<SshHandler>,
    base: &'a Path,
    current: &'a Path,
    remote_base: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Box<dyn std::error::Error>>> + 'a>>
{
    Box::pin(async move {
        for entry in std::fs::read_dir(current)
            .map_err(|e| format!("cannot read directory {:?}: {}", current, e))?
        {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap();
            let remote_path = format!("{}/{}", remote_base, rel.to_string_lossy());

            if path.is_dir() {
                exec(session, &format!("mkdir -p {}", remote_path)).await?;
                upload_dir_recursive(session, base, &path, remote_base).await?;
            } else {
                // Skip if already present
                if !remote_exists(session, &remote_path).await? {
                    upload_file(
                        session,
                        path.to_str().ok_or_else(|| {
                            format!("file path contains invalid UTF-8: {:?}", path)
                        })?,
                        &remote_path,
                        false,
                        false,
                    )
                    .await?;
                } else {
                    info!("[provision] skip upload (already exists): {}", remote_path);
                }
            }
        }
        Ok(())
    })
}

// ── Provisioning Steps ────────────────────────────────────────────────────────

/// Detect which package manager is available on the remote host.
async fn detect_pkg_manager(
    session: &mut client::Handle<SshHandler>,
) -> Result<String, Box<dyn std::error::Error>> {
    for pm in &["apt-get", "apk", "dnf", "yum"] {
        let res = exec(session, &format!("command -v {} >/dev/null 2>&1", pm)).await?;
        if res.success() {
            return Ok(pm.to_string());
        }
    }
    Err("no supported package manager found (tried apt-get, apk, dnf, yum)".into())
}

/// Install all required system packages, skipping those that are already present.
async fn install_deps(
    session: &mut client::Handle<SshHandler>,
    pkg_mgr: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Binaries that must be present after installation
    struct Dep {
        binary: &'static str,    // `command -v <binary>` check
        apt_pkg: &'static str,
        apk_pkg: &'static str,
        yum_pkg: &'static str,
    }

    let deps = [
        Dep { binary: "postfix",    apt_pkg: "postfix",          apk_pkg: "postfix",          yum_pkg: "postfix" },
        Dep { binary: "dovecot",    apt_pkg: "dovecot-core",     apk_pkg: "dovecot",          yum_pkg: "dovecot" },
        Dep { binary: "opendkim",   apt_pkg: "opendkim",         apk_pkg: "opendkim",         yum_pkg: "opendkim" },
        Dep { binary: "openssl",    apt_pkg: "openssl",          apk_pkg: "openssl",          yum_pkg: "openssl" },
        Dep { binary: "curl",       apt_pkg: "curl",             apk_pkg: "curl",             yum_pkg: "curl" },
        Dep { binary: "psql",       apt_pkg: "postgresql-client",apk_pkg: "postgresql-client",yum_pkg: "postgresql" },
    ];

    // Update package index once (only for apt/apk)
    match pkg_mgr {
        "apt-get" => {
            run_remote(session, "apt-get update", "DEBIAN_FRONTEND=noninteractive apt-get update -qq").await?;
        }
        "apk" => {
            run_remote(session, "apk update", "apk update -q").await?;
        }
        _ => {}
    }

    for dep in &deps {
        if command_exists(session, dep.binary).await? {
            info!("[provision] skip: {} already installed", dep.binary);
            continue;
        }

        let pkg = match pkg_mgr {
            "apt-get" => dep.apt_pkg,
            "apk" => dep.apk_pkg,
            _ => dep.yum_pkg,
        };

        let install_cmd = match pkg_mgr {
            "apt-get" => format!("DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {}", pkg),
            "apk" => format!("apk add --quiet {}", pkg),
            "dnf" => format!("dnf install -y -q {}", pkg),
            _ => format!("yum install -y -q {}", pkg),
        };

        let rc = run_remote(session, &format!("install {}", pkg), &install_cmd).await?;
        if rc != 0 {
            error!("[provision] failed to install package '{}' (exit {})", pkg, rc);
            return Err(format!("package install failed: {}", pkg).into());
        }
    }

    // Extra dovecot sub-packages for LMTP
    let lmtp_check = match pkg_mgr {
        "apk" => "test -f /usr/lib/dovecot/lmtp",
        "apt-get" => "dpkg -l dovecot-lmtpd 2>/dev/null | grep -q '^ii'",
        _ => "test -f /usr/libexec/dovecot/lmtp",
    };

    if !exec(session, lmtp_check).await?.success() {
        let lmtp_pkg = match pkg_mgr {
            "apt-get" => "dovecot-lmtpd dovecot-imapd dovecot-pop3d",
            "apk" => "dovecot-lmtpd dovecot-pigeonhole-plugin",
            _ => "dovecot",
        };
        let install_cmd = match pkg_mgr {
            "apt-get" => format!("DEBIAN_FRONTEND=noninteractive apt-get install -y -qq {}", lmtp_pkg),
            "apk" => format!("apk add --quiet {}", lmtp_pkg),
            "dnf" => format!("dnf install -y -q {}", lmtp_pkg),
            _ => format!("yum install -y -q {}", lmtp_pkg),
        };
        run_remote(session, "install dovecot LMTP/IMAP/POP3 plugins", &install_cmd).await?;
    } else {
        info!("[provision] skip: dovecot LMTP already installed");
    }

    Ok(())
}

/// Create the vmail and opendkim system users and all required directories.
async fn setup_users_and_dirs(
    session: &mut client::Handle<SshHandler>,
    pkg_mgr: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // User creation helpers differ between distros
    let (addgroup_flags, adduser_flags) = match pkg_mgr {
        "apk" => ("-S", "-S -D -H -s /sbin/nologin"),
        _ => ("--system", "--system --no-create-home --shell /usr/sbin/nologin"),
    };

    for (user, group) in &[("vmail", "vmail"), ("opendkim", "opendkim")] {
        let exists = exec(session, &format!("id {} >/dev/null 2>&1", user))
            .await?
            .success();
        if exists {
            info!("[provision] skip: user {} already exists", user);
        } else {
            // Create group then user
            exec(
                session,
                &format!("groupadd {} {} 2>/dev/null || addgroup {} {} 2>/dev/null || true",
                    addgroup_flags, group, addgroup_flags, group),
            )
            .await?;
            run_remote(
                session,
                &format!("create system user {}", user),
                &format!(
                    "useradd {} -g {} {} 2>/dev/null || adduser {} -G {} {} 2>/dev/null || true",
                    adduser_flags, group, user,
                    adduser_flags, group, user
                ),
            )
            .await?;
        }
    }

    // Required directories
    let dirs = [
        "/data/ssl",
        "/data/dkim",
        "/data/mail",
        "/data/db",
        "/var/spool/postfix",
        "/app/templates/config",
        "/app/migrations",
        "/app/static",
        "/etc/mailserver",
        "/usr/local/bin",
    ];

    for dir in &dirs {
        if remote_exists(session, dir).await? {
            info!("[provision] skip: directory {} already exists", dir);
        } else {
            run_remote(session, &format!("create directory {}", dir), &format!("mkdir -p {}", dir)).await?;
        }
    }

    // Ownership
    run_remote(session, "chown /data/mail → vmail", "chown -R vmail:vmail /data/mail").await?;
    run_remote(session, "chown /data/dkim → opendkim", "chown -R opendkim:opendkim /data/dkim").await?;

    Ok(())
}

/// Upload the currently-running mailserver binary to the remote host.
async fn upload_binary(
    session: &mut client::Handle<SshHandler>,
) -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot determine current executable path: {}", e))?;
    let exe_str = exe
        .to_str()
        .ok_or("current executable path contains invalid UTF-8")?;

    info!("[provision] current binary: {}", exe_str);

    // Always overwrite the binary so the remote gets the current version.
    // (skip_if_exists = false)
    upload_file(session, exe_str, "/usr/local/bin/mailserver", true, false).await?;

    Ok(())
}

/// Upload `templates/`, `migrations/`, `static/`, and `entrypoint.sh` to the
/// remote host.  Each file is skipped if it already exists.
async fn upload_support_files(
    session: &mut client::Handle<SshHandler>,
) -> Result<(), Box<dyn std::error::Error>> {
    // entrypoint.sh
    if Path::new("entrypoint.sh").exists() {
        upload_file(session, "entrypoint.sh", "/entrypoint.sh", true, true).await?;
    } else if Path::new("/app/entrypoint.sh").exists() {
        upload_file(session, "/app/entrypoint.sh", "/entrypoint.sh", true, true).await?;
    } else {
        info!("[provision] entrypoint.sh not found locally, skipping");
    }

    // Template config files
    for local_dir in &["templates/config", "/app/templates/config"] {
        if Path::new(local_dir).exists() {
            upload_dir(session, local_dir, "/app/templates/config").await?;
            break;
        }
    }

    // Migrations
    for local_dir in &["migrations", "/app/migrations"] {
        if Path::new(local_dir).exists() {
            upload_dir(session, local_dir, "/app/migrations").await?;
            break;
        }
    }

    // Static assets
    for local_dir in &["static", "/app/static"] {
        if Path::new(local_dir).exists() {
            upload_dir(session, local_dir, "/app/static").await?;
            break;
        }
    }

    Ok(())
}

/// Run initial one-time setup commands on the remote host.  Each command is
/// guarded by a quick existence check so it is skipped if already done.
async fn initial_setup(
    session: &mut client::Handle<SshHandler>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Source the env file if present; otherwise warn and continue
    let env_prefix = "set -a && [ -f /etc/mailserver/env ] && . /etc/mailserver/env; set +a;";

    // TLS certificates
    if remote_exists(session, "/data/ssl/cert.pem").await?
        && remote_exists(session, "/data/ssl/key.pem").await?
    {
        info!("[provision] skip: TLS certificates already exist");
    } else {
        run_remote(
            session,
            "generate TLS certificates",
            &format!("{} /usr/local/bin/mailserver gencerts", env_prefix),
        )
        .await?;
    }

    // Database seed (idempotent — mailserver seed is safe to re-run)
    run_remote(
        session,
        "seed admin user",
        &format!("{} /usr/local/bin/mailserver seed", env_prefix),
    )
    .await?;

    // Generate mail service configs
    run_remote(
        session,
        "generate mail configs",
        &format!("{} /usr/local/bin/mailserver genconfig", env_prefix),
    )
    .await?;

    Ok(())
}

/// Write and enable the system service definition (systemd or OpenRC).
async fn setup_service(
    session: &mut client::Handle<SshHandler>,
    _pkg_mgr: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Detect init system
    let has_systemd = command_exists(session, "systemctl").await?;
    let has_openrc = command_exists(session, "rc-update").await?;

    if has_systemd {
        if remote_exists(session, "/etc/systemd/system/mailserver.service").await? {
            info!("[provision] skip: systemd unit already installed");
        } else {
            let unit = SYSTEMD_UNIT;
            write_remote_text(session, "/etc/systemd/system/mailserver.service", unit).await?;
            run_remote(session, "reload systemd daemon", "systemctl daemon-reload").await?;
            info!("[provision] systemd unit installed");
        }
    } else if has_openrc {
        if remote_exists(session, "/etc/init.d/mailserver").await? {
            info!("[provision] skip: OpenRC init script already installed");
        } else {
            // Use apk-style openrc init script
            write_remote_text(session, "/etc/init.d/mailserver", OPENRC_INIT).await?;
            run_remote(session, "chmod openrc init script", "chmod +x /etc/init.d/mailserver").await?;
            info!("[provision] OpenRC init script installed");
        }
    } else {
        warn!("[provision] neither systemd nor OpenRC detected; manual service configuration required");
    }

    Ok(())
}

/// Enable and start (or restart) the mailserver service.
async fn start_service(
    session: &mut client::Handle<SshHandler>,
    _pkg_mgr: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let has_systemd = command_exists(session, "systemctl").await?;
    let has_openrc = command_exists(session, "rc-update").await?;

    if has_systemd {
        run_remote(session, "enable mailserver service", "systemctl enable mailserver").await?;
        run_remote(session, "restart mailserver service", "systemctl restart mailserver").await?;
        run_remote(session, "service status", "systemctl is-active mailserver || true").await?;
    } else if has_openrc {
        run_remote(session, "add to default runlevel", "rc-update add mailserver default 2>/dev/null || true").await?;
        run_remote(session, "start mailserver service", "rc-service mailserver restart || true").await?;
    } else {
        warn!("[provision] cannot start service automatically; start /entrypoint.sh manually");
    }

    Ok(())
}

// ── Text File Upload Helper ───────────────────────────────────────────────────

/// Write a UTF-8 string to a remote file using an exec channel.
async fn write_remote_text(
    session: &mut client::Handle<SshHandler>,
    remote_path: &str,
    content: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Ensure parent directory exists
    if let Some(parent) = Path::new(remote_path).parent() {
        let p = parent.to_string_lossy();
        if !p.is_empty() && p != "/" {
            exec(session, &format!("mkdir -p {}", p)).await?;
        }
    }

    let encoded = BASE64.encode(content.as_bytes());
    let cmd = format!("printf '%s' '{}' | base64 -d > {}", encoded, remote_path);
    let res = exec(session, &cmd).await?;
    if !res.success() {
        return Err(format!("failed to write {}: exit {}", remote_path, res.exit_code).into());
    }
    info!("[provision] ✓ wrote {}", remote_path);
    Ok(())
}

// ── Service Unit Definitions ──────────────────────────────────────────────────

const SYSTEMD_UNIT: &str = r#"[Unit]
Description=Mailserver (Postfix + Dovecot + OpenDKIM managed by mailserver binary)
After=network.target
Wants=network.target

[Service]
Type=simple
EnvironmentFile=-/etc/mailserver/env
ExecStart=/entrypoint.sh
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
"#;

const OPENRC_INIT: &str = r#"#!/sbin/openrc-run

name="mailserver"
description="Mailserver (Postfix + Dovecot + OpenDKIM)"
command="/entrypoint.sh"
command_background=true
pidfile="/run/${RC_SVCNAME}.pid"

depend() {
    need net
}
"#;

// ── Usage ─────────────────────────────────────────────────────────────────────

pub fn print_usage() {
    println!("Usage:");
    println!("  mailserver provision --host <host> --user <user> [options]");
    println!();
    println!("Options:");
    println!("  --host <host>         Remote host name or IP address (required)");
    println!("  --port <port>         SSH port (default: 22)");
    println!("  --user <user>         SSH login username (required)");
    println!("  --key <path>          Path to SSH private key file (recommended)");
    println!("  --password <pwd>      Password for SSH auth or encrypted key passphrase");
    println!("  --env-file <path>     Local .env file to upload as /etc/mailserver/env");
    println!();
    println!("The command connects via SSH, installs system dependencies, uploads the");
    println!("current mailserver binary and supporting files, configures the system");
    println!("service, and starts it. Steps that are already done are automatically");
    println!("skipped.  Credentials are only kept in memory and never written to disk.");
    println!();
    println!("Examples:");
    println!("  mailserver provision --host mail.example.com --user root --key ~/.ssh/id_ed25519");
    println!("  mailserver provision --host 10.0.0.5 --user admin --key ~/.ssh/id_rsa --password mypass");
    println!("  mailserver provision --host mail.example.com --user root --password s3cr3t --env-file .env.prod");
}
