use bcrypt::{hash, verify, DEFAULT_COST};
use data_encoding::BASE32;
use hmac::{Hmac, Mac};
use rand::Rng;
use sha1::Sha1;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha1 = Hmac<Sha1>;

pub fn verify_password(password: &str, hash: &str) -> bool {
    verify(password, hash).unwrap_or(false)
}

pub fn hash_password(password: &str) -> String {
    hash(password, DEFAULT_COST).expect("failed to hash password")
}

pub fn generate_totp_secret() -> String {
    let mut rng = rand::thread_rng();
    let secret: Vec<u8> = (0..20).map(|_| rng.gen::<u8>()).collect();
    BASE32.encode(&secret)
}

pub fn verify_totp(secret_base32: &str, code: &str) -> bool {
    let secret = match BASE32.decode(secret_base32.as_bytes()) {
        Ok(s) => s,
        Err(_) => return false,
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
            return true;
        }
    }
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
    format!(
        "otpauth://totp/Mailserver:{}?secret={}&issuer=Mailserver",
        username, secret
    )
}

pub fn generate_dovecot_password(password: &str) -> String {
    let bcrypt_hash = hash(password, DEFAULT_COST).expect("failed to hash password");
    format!("{{BLF-CRYPT}}{}", bcrypt_hash)
}
