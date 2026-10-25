// Firma e cifratura delle chiamate cloud Tuya OEM â€” replica verificata
// contro jnimain.dll (oracolo nativo 32-bit, vedi tests/oracle_jnimain).
//
// Algoritmi estratti dal bundle LSC 2.0.17:
//   sigStrCache = "A_yegjfwuukevd8qfxw3rjfr5sj43p5gpr_xh59e8qykn4sp7jyh7rwaq3ykfwf8e5n"
//                 (segreto lista dalla BMP t_s_online.bmp + "_" + appSecret)
//   sign(str)   = hex(HMAC_SHA256(key=sigStrCache, msg=str))            [64 char]
//   secret(rid) = hex(HMAC_SHA256(key=rid, msg=sigStrCache [+ "_" + ecode]))[..16]
//   postData    = base64(AES-128-ECB/PKCS7(json, key=secret_ascii))
use crate::proto::crypto;
use aes::Aes128;
use cipher::{BlockDecryptMut, BlockEncryptMut, KeyInit};
use md5::{Digest as _, Md5};

pub const SIG_STR_CACHE: &str =
    "A_yegjfwuukevd8qfxw3rjfr5sj43p5gpr_xh59e8qykn4sp7jyh7rwaq3ykfwf8e5n";

/// Replica di swapSignString: riordino "errato" dell'MD5 del postData.
pub fn swap_sign_string(data: &str) -> String {
    let first = &data[0..8];
    let middle = &data[8..24];
    let middle1 = &middle[0..8];
    let middle2 = &middle[8..16];
    let last = &data[24..32];
    format!("{}{}{}{}", middle1, first, last, middle2)
}

pub fn md5_hex(data: &str) -> String {
    let mut h = Md5::new();
    h.update(data.as_bytes());
    hex::encode(h.finalize())
}

/// Firma di una richiesta: stringa "k=v||..." (ordinata, con postData
/// sostituito dall'MD5 riordinato) â†’ HMAC esadecimale.
pub fn sign_params(pairs: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = pairs.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut s = String::new();
    for (k, v) in &sorted {
        if k == "postData" {
            if !v.is_empty() {
                s.push_str(&format!("postData={}||", swap_sign_string(&md5_hex(v))));
            }
        } else {
            s.push_str(&format!("{}={}||", k, v));
        }
    }
    let s = s.trim_end_matches("||");
    hex::encode(crypto::hmac_sha256(SIG_STR_CACHE.as_bytes(), s.as_bytes()))
}

/// Chiave AES (16 char ASCII) per cifrare/decifrare postData e result.
pub fn post_secret(request_id: &str, ecode_key: Option<&str>) -> String {
    let msg = match ecode_key {
        Some(k) => format!("{}_{}", SIG_STR_CACHE, k),
        None => SIG_STR_CACHE.to_string(),
    };
    let h = crypto::hmac_sha256(request_id.as_bytes(), msg.as_bytes());
    hex::encode(&h[..8])
}

fn pkcs7_pad(data: &[u8]) -> Vec<u8> {
    let pad = 16 - (data.len() % 16);
    let mut v = data.to_vec();
    v.extend(std::iter::repeat(pad as u8).take(pad));
    v
}

pub fn aes_ecb_encrypt_base64(plaintext: &str, key16_ascii: &str) -> String {
    let key = key16_ascii.as_bytes();
    let mut cipher = Aes128::new_from_slice(key).expect("key aes");
    let mut buf = pkcs7_pad(plaintext.as_bytes());
    for chunk in buf.chunks_exact_mut(16) {
        cipher.encrypt_block_mut(chunk.into());
    }
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(&buf)
}

pub fn aes_ecb_decrypt_base64(ciphertext_b64: &str, key16_ascii: &str) -> Option<String> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(ciphertext_b64)
        .ok()?;
    let mut cipher = Aes128::new_from_slice(key16_ascii.as_bytes()).ok()?;
    let mut buf = raw;
    for chunk in buf.chunks_exact_mut(16) {
        cipher.decrypt_block_mut(chunk.into());
    }
    if buf.is_empty() {
        return None;
    }
    let pad = *buf.last()? as usize;
    if pad == 0 || pad > 16 || pad > buf.len() {
        return None;
    }
    buf.truncate(buf.len() - pad);
    String::from_utf8(buf).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tutti i casi dell'oracolo jnimain.dll (tests/oracle_jnimain/cases.md):
    // firma HMAC-SHA256 a 64 char, larghezza piena.
    const SIGN_CASES: &[(&str, &str)] = &[
        ("", "b42a11c88c37af33b5cb6752a54d92ec5b7d034d3f499bd5c5fea39414eb4963"),
        ("a", "b17a597fde294a1818dc44574c9baf1a544bd95b3e64ca07206acfd5e4239950"),
        ("a=1||b=2", "38cf9902909887f50ea01987867eb69c4bde59f8da346185047962ab3fa777e8"),
        (
            "appRnVersion=5.27||appVersion=1.0.0||channel=oem||clientId=q594qaqdpy89gmvyndtp",
            "cb24c2f4ead05558c75169b69c5ea917fb6017a6dff7c14629d01cf5a3dc95ca",
        ),
        (
            "postData=0123456789abcdef0123456789abcdef||t=1700000000",
            "2e2f18d97c6de03936a79c9dc1ef81c03e00d023265788c897c0d3637d7b82a1",
        ),
        (
            "a=1||b=2||c=3||d=4||e=5||f=6||g=7",
            "fbb2718d47fe872b12f2caa9179bdf99d897fa61a28a7d2fd2dc83f7e33eb384",
        ),
        (
            "tuya.m.user.qr.token.create",
            "fe4c36250c5d5b6e7e908127fb45107c28e6bb81bf46f2c0e389224f6849fa94",
        ),
        (
            "requestId=6f6a2f9b8d7c4e5f9a1b2c3d4e5f6789||sid=abc123",
            "f5eb181d7bcbcc1164a1deecf36fe5aa437fca4864eb704113d1b5dec15bc173",
        ),
    ];

    #[test]
    fn sign_matches_oracle() {
        for (input, expected) in SIGN_CASES {
            let got = hex::encode(crypto::hmac_sha256(SIG_STR_CACHE.as_bytes(), input.as_bytes()));
            assert_eq!(got, *expected, "sign({}) diversa dall'oracolo", input);
        }
    }

    #[test]
    fn post_secret_matches_oracle() {
        assert_eq!(post_secret("00000000-0000-4000-8000-000000000000", None), "7ca1c563b051af7a");
        assert_eq!(post_secret("12345678-1234-4123-8123-123456789abc", None), "f18580c627b4a2f2");
        assert_eq!(post_secret("abcdef01-2345-6789-abcd-ef0123456789", None), "415275f8213c175c");
        assert_eq!(
            post_secret("00000000-0000-4000-8000-000000000000", Some("ecodekeyvalue")),
            "749418ad952880a8"
        );
        assert_eq!(
            post_secret("12345678-1234-4123-8123-123456789abc", Some("ecodekeyvalue")),
            "b68d9d3e99315714"
        );
        assert_eq!(
            post_secret("abcdef01-2345-6789-abcd-ef0123456789", Some("ecodekeyvalue")),
            "9876edd257c5a57e"
        );
    }

    #[test]
    fn swap_sign_string_reorders() {
        // first=data[0..8], result=middle1+first+last+middle2 = d[8..16]+d[0..8]+d[24..32]+d[16..24]
        let d = "0123456789abcdef0123456789abcdef";
        assert_eq!(swap_sign_string(d), "89abcdef0123456789abcdef01234567");
    }

    #[test]
    fn aes_ecb_roundtrip() {
        let key = "0123456789abcdef"; // 16 char ASCII = 16 byte
        let ct = aes_ecb_encrypt_base64("A", key);
        let pt = aes_ecb_decrypt_base64(&ct, key).unwrap();
        assert_eq!(pt, "A");
    }

    #[test]
    fn sign_params_full_request_string() {
        let pairs = vec![
            ("a".to_string(), "tuya.m.user.qr.token.create".to_string()),
            ("v".to_string(), "1.0".to_string()),
        ];
        let s = sign_params(&pairs);
        assert_eq!(s.len(), 64);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

