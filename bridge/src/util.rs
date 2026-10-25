// Helper che replicano esattamente `src/utils/parseData.js` dell'app LSC 2.0.17.
use sha2::{Digest, Sha256};

/// Replica getByteDataFromLen(len_bytes, data, isEnd): pad a stringa hex a `len*2` caratteri.
pub fn pad_hex(len: usize, data: &str, end: bool) -> String {
    let mut data = data.to_string();
    while data.len() < len * 2 {
        if end {
            data.push('0');
        } else {
            data.insert(0, '0');
        }
    }
    data
}

pub fn hex_encode(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

pub fn hex_decode(s: &str) -> Result<Vec<u8>, hex::FromHexError> {
    hex::decode(s)
}

/// Replica stringtoHex: ogni carattere della stringa → 2 cifre hex del suo charCode.
pub fn string_to_hex(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        out.push_str(&format!("{:02x}", c as u32));
    }
    out
}

/// Replica getRamNumberBuffer(type="macId"): sha256(macHex + "macId")[:8] come stringa hex.
pub fn mac_uuid_hex(mac_hex: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{}macId", mac_hex).as_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest[..8])
}

pub fn sha256_seed(seed: &str, len: usize) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    hasher.finalize()[..len].to_vec()
}

/// CRC32 IEEE reflected → 8 char hex unsigned big-endian (vedi ruling nel ledger).
pub fn crc32_hex(data: &[u8]) -> String {
    let mut h = crc32fast::Hasher::new();
    h.update(data);
    format!("{:08x}", h.finalize())
}

pub fn crc32_bytes(data: &[u8]) -> [u8; 4] {
    let mut h = crc32fast::Hasher::new();
    h.update(data);
    h.finalize().to_be_bytes()
}

pub fn xor(a: &[u8], b: &[u8]) -> Vec<u8> {
    a.iter().zip(b.iter()).map(|(x, y)| x ^ y).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_hex_pads_left_by_default() {
        assert_eq!(pad_hex(2, "ff", false), "00ff");
    }

    #[test]
    fn pad_hex_pads_right_when_end() {
        assert_eq!(pad_hex(2, "ff", true), "ff00");
    }

    #[test]
    fn pad_hex_leaves_longer_untouched() {
        assert_eq!(pad_hex(1, "ff", false), "ff");
    }

    #[test]
    fn string_to_hex_doubles_chars() {
        assert_eq!(string_to_hex("ab"), "6162");
    }

    #[test]
    fn crc32_matches_known_vector() {
        // buffer-crc32 v2+ / zlib: 0x3610a686
        assert_eq!(crc32_hex(b"hello"), "3610a686");
    }
}
