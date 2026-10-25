// Derivazione mate_id (replica di getMainMateId / getChildMateId in
// src/sdk/localUdpNetwork/index.js dell'app originale).
use crate::util;

/// mainMateId = crc32( pad25(stringToHex(mainUuidHex)) + "00" ) → 4 byte.
pub fn main_mate_id(main_uuid_hex: &str) -> [u8; 4] {
    let uuid = util::pad_hex(25, &util::string_to_hex(main_uuid_hex), true);
    let data = util::hex_decode(&format!("{}00", uuid)).unwrap();
    util::crc32_bytes(&data)
}

/// childMateId = crc32( pad25(stringToHex(devId)) + "00" + mainMateId ) → 4 byte.
pub fn child_mate_id(dev_id: &str, main_mate_id: &[u8; 4]) -> [u8; 4] {
    let dev = util::pad_hex(25, &util::string_to_hex(dev_id), true);
    let mut data = util::hex_decode(&format!("{}00", dev)).unwrap();
    data.extend_from_slice(main_mate_id);
    util::crc32_bytes(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_mate_id_is_four_bytes() {
        assert_eq!(main_mate_id("aabbccddeeff0011").len(), 4);
    }
}
