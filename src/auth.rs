use bcrypt::{hash, verify, DEFAULT_COST};
use data_encoding::BASE32;
use hmac::{Hmac, Mac};
use log::{debug, error, info, warn};
use rand::Rng;
use sha1::Sha1;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha1 = Hmac<Sha1>;

pub fn verify_password(password: &str, hash: &str) -> bool {
    debug!("[auth] verifying password hash");
    let result = verify(password, hash).unwrap_or(false);
    if result {
        debug!("[auth] password verification succeeded");
    } else {
        warn!("[auth] password verification failed");
    }
    result
}

pub fn hash_password(password: &str) -> String {
    debug!("[auth] hashing password with bcrypt cost={}", DEFAULT_COST);
    let result = hash(password, DEFAULT_COST).expect("failed to hash password");
    debug!("[auth] password hashed successfully");
    result
}

pub fn generate_totp_secret() -> String {
    info!("[auth] generating new TOTP secret");
    let mut rng = rand::thread_rng();
    let secret: Vec<u8> = (0..20).map(|_| rng.gen::<u8>()).collect();
    let encoded = BASE32.encode(&secret);
    debug!("[auth] TOTP secret generated (length={})", encoded.len());
    encoded
}

pub fn verify_totp(secret_base32: &str, code: &str) -> bool {
    debug!("[auth] verifying TOTP code");
    let secret = match BASE32.decode(secret_base32.as_bytes()) {
        Ok(s) => s,
        Err(e) => {
            error!("[auth] failed to decode TOTP secret: {}", e);
            return false;
        }
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_secs();
    let current_step = now / 30;

    for offset in [0, 1, u64::MAX] {
        let step = current_step.wrapping_add(offset);
        let generated = totp_code(&secret, step);
        if generated == code {
            info!("[auth] TOTP verification succeeded (offset={})", offset);
            return true;
        }
    }
    warn!("[auth] TOTP verification failed — code did not match any window");
    false
}

fn totp_code(secret: &[u8], step: u64) -> String {
    let step_bytes = step.to_be_bytes();
    let mut mac = HmacSha1::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&step_bytes);
    let result = mac.finalize().into_bytes();

    let offset = (result[19] & 0x0f) as usize;
    let code = u32::from_be_bytes([
        result[offset] & 0x7f,
        result[offset + 1],
        result[offset + 2],
        result[offset + 3],
    ]) % 1_000_000;

    format!("{:06}", code)
}

pub fn totp_uri(secret: &str, username: &str) -> String {
    debug!("[auth] generating TOTP URI for username={}", username);
    format!(
        "otpauth://totp/Mailserver:{}?secret={}&issuer=Mailserver",
        username, secret
    )
}
