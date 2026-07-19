use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub fn verify_signature(secret: &str, body: &[u8], signature: &str) -> bool {
    let expected_prefix = "sha256=";
    if !signature.starts_with(expected_prefix) {
        return false;
    }
    let sig_hex = &signature[expected_prefix.len()..];

    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(body);
    let computed = hex::encode(mac.finalize().into_bytes());

    // Constant-time comparison to prevent timing side-channel attacks
    bool::from(computed.as_bytes().ct_eq(sig_hex.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_hmac_verifies() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let secret = "my-secret";
        let body = b"hello world";
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let sig = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(verify_signature(secret, body, &sig));
    }

    #[test]
    fn wrong_secret_fails() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let body = b"data";
        let mut mac = Hmac::<Sha256>::new_from_slice(b"wrong-key").unwrap();
        mac.update(body);
        let sig = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(!verify_signature("real-key", body, &sig));
    }

    #[test]
    fn missing_prefix_fails() {
        assert!(!verify_signature("secret", b"body", "abcdef"));
    }

    #[test]
    fn empty_secret_fails() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let body = b"test";
        // HMAC with non-empty key won't match empty-key HMAC
        let mut mac = Hmac::<Sha256>::new_from_slice(b"real-secret").unwrap();
        mac.update(body);
        let sig = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(!verify_signature("", body, &sig));
    }
}
