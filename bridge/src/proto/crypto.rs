// Crittografia del protocollo (AES-128-GCM + HMAC-SHA256), replica di
// src/sdk/localUdpNetwork/crypto.js e della negoziazione in index.js.
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::Aes128Gcm;
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const GCM_TAG_LEN: usize = 16;

fn cipher(key: &[u8]) -> Aes128Gcm {
    Aes128Gcm::new(GenericArray::from_slice(key))
}

fn to_nonce(n: &[u8]) -> GenericArray<u8, <Aes128Gcm as aes_gcm::aead::AeadCore>::NonceSize> {
    *GenericArray::from_slice(&n[..12])
}

/// encodeGCM: ritorna (ciphertext, tag).
pub fn gcm_encrypt(key: &[u8], nonce: &[u8], aad: &[u8], plaintext: &[u8]) -> (Vec<u8>, [u8; 16]) {
    let c = cipher(key);
    let payload = Payload { msg: plaintext, aad };
    let ct = c.encrypt(&to_nonce(nonce), payload).expect("gcm encrypt");
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&ct[ct.len() - 16..]);
    (ct[..ct.len() - 16].to_vec(), tag)
}

/// decodeGCM: Some(plaintext) se il tag è valido.
pub fn gcm_decrypt(key: &[u8], nonce: &[u8], aad: &[u8], ciphertext: &[u8], tag: &[u8]) -> Option<Vec<u8>> {
    if tag.len() != GCM_TAG_LEN {
        return None;
    }
    let c = cipher(key);
    let mut combined = ciphertext.to_vec();
    combined.extend_from_slice(tag);
    c.decrypt(&to_nonce(nonce), Payload { msg: &combined, aad }).ok()
}

pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac key");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

/// negotiate_key = AES128GCM(token, iv=token[0..12], key=localKey) primi 16 byte.
pub fn negotiate_key(local_key: &[u8], token: &[u8]) -> Vec<u8> {
    let (ct, _tag) = gcm_encrypt(local_key, &token[..12], &[], token);
    ct[..16].to_vec()
}

/// session_key = AES128GCM(randA XOR randB, iv=randA[0..12], key=negotiate_key) primi 16 byte.
pub fn session_key(negotiate_key: &[u8], rand_a: &[u8], rand_ab: &[u8]) -> Vec<u8> {
    let (ct, _tag) = gcm_encrypt(negotiate_key, &rand_a[..12], &[], rand_ab);
    ct[..16].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcm_roundtrip() {
        let key = [1u8; 16];
        let nonce = [2u8; 12];
        let (ct, tag) = gcm_encrypt(&key, &nonce, b"aad", b"segretto");
        let pt = gcm_decrypt(&key, &nonce, b"aad", &ct, &tag).expect("decrypt");
        assert_eq!(pt, b"segretto");
    }

    #[test]
    fn gcm_rejects_bad_tag() {
        let key = [1u8; 16];
        let nonce = [2u8; 12];
        let (ct, _tag) = gcm_encrypt(&key, &nonce, b"", b"data");
        assert!(gcm_decrypt(&key, &nonce, b"", &ct, &[0u8; 16]).is_none());
    }
}
