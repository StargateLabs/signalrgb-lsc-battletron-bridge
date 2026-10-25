// Struttura dei frame UDP (replica di SendBroadcast/createAd/createBusinessData
// in src/sdk/localUdpNetwork/index.js e del parser online in connectDevice.js).
use crate::proto::crypto;
use crate::util;

pub const HEADER: [u8; 4] = [0x00, 0x00, 0x66, 0x99];
pub const TAIL: [u8; 4] = [0x00, 0x00, 0x99, 0x66];
const DATA_LENGTH_BASE: u32 = 42; // header→tail esclusi i dati di business

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PutType {
    ConsultRequest,  // 0x05
    ConsultResponse, // 0x06
    BusinessRequest, // 0x10
}

impl PutType {
    pub fn code(self) -> [u8; 4] {
        match self {
            PutType::ConsultRequest => [0, 0, 0, 0x05],
            PutType::ConsultResponse => [0, 0, 0, 0x06],
            PutType::BusinessRequest => [0, 0, 0, 0x10],
        }
    }
}

pub struct ConsultEntry {
    pub mate_id: [u8; 4],
    pub cipher: Vec<u8>,
    pub tag: [u8; 16],
}

pub struct BusinessItem {
    pub mate_id: [u8; 4],
    pub cipher: Vec<u8>,
    pub tag: [u8; 16],
}

/// add (22 byte): 00 00 | seq(4, BE) | type(4) | mainMateId(4) | totalLen(4, BE) | frameNum(4, BE)
pub fn build_add(
    put: PutType,
    entries_len: usize,
    seq: u32,
    main_mate_id: &[u8; 4],
    payload_len: u32,
) -> Vec<u8> {
    let total_len = DATA_LENGTH_BASE + payload_len;
    let seq_hex = util::pad_hex(4, &format!("{:x}", seq), false);
    let total_hex = util::pad_hex(4, &format!("{:x}", total_len), false);
    let frame_num = util::pad_hex(4, &format!("{:x}", entries_len), false);
    let mut add = Vec::with_capacity(22);
    add.extend_from_slice(&hex::decode(format!(
        "0000{}{}",
        seq_hex,
        hex::encode(put.code())
    ))
    .unwrap());
    add.extend_from_slice(main_mate_id);
    add.extend_from_slice(&hex::decode(format!("{}{}", total_hex, frame_num)).unwrap());
    add
}

fn device_block(mate_id: [u8; 4], cipher: &[u8], tag: &[u8; 16]) -> Vec<u8> {
    let len_hex = util::pad_hex(4, &format!("{:x}", cipher.len()), false);
    let mut b = Vec::with_capacity(4 + 4 + cipher.len() + 16);
    b.extend_from_slice(&mate_id);
    b.extend_from_slice(&hex::decode(len_hex).unwrap());
    b.extend_from_slice(cipher);
    b.extend_from_slice(tag);
    b
}

fn payload_len_of(entries: usize, ciphers: &[Vec<u8>]) -> u32 {
    let _ = entries;
    ciphers.iter().map(|c| c.len() as u32 + 24).sum()
}

pub fn build_consult_request(
    seq: u32,
    nonce: &[u8],
    main_mate_id: &[u8; 4],
    entries: &[ConsultEntry],
) -> Vec<u8> {
    let payload_len = payload_len_of(entries.len(), &entries.iter().map(|e| e.cipher.clone()).collect::<Vec<_>>());
    let add = build_add(PutType::ConsultRequest, entries.len(), seq, main_mate_id, payload_len);
    let mut frame = Vec::new();
    frame.extend_from_slice(&HEADER);
    frame.extend_from_slice(&add);
    frame.extend_from_slice(nonce);
    for e in entries {
        frame.extend_from_slice(&device_block(e.mate_id, &e.cipher, &e.tag));
    }
    frame.extend_from_slice(&TAIL);
    frame
}

pub fn build_business_request(
    seq: u32,
    nonce: &[u8],
    main_mate_id: &[u8; 4],
    items: &[BusinessItem],
) -> Vec<u8> {
    let payload_len = payload_len_of(items.len(), &items.iter().map(|i| i.cipher.clone()).collect::<Vec<_>>());
    let add = build_add(PutType::BusinessRequest, items.len(), seq, main_mate_id, payload_len);
    let mut frame = Vec::new();
    frame.extend_from_slice(&HEADER);
    frame.extend_from_slice(&add);
    frame.extend_from_slice(nonce);
    for i in items {
        frame.extend_from_slice(&device_block(i.mate_id, &i.cipher, &i.tag));
    }
    frame.extend_from_slice(&TAIL);
    frame
}

/// Frame di risposta device (type 0x06): header(4) + add(22, mate a [14..18])
/// + nonce(12) + id(4, ignorato) + len(4) + cipher + tag(16) + tail(4).
/// (layout da dealData dell'app: mate@14..18, nonce@26..38, id@38..42,
///  length@42..46, cipher@46..)
pub fn parse_reply(data: &[u8]) -> Option<([u8; 4], Vec<u8>, [u8; 16])> {
    if data.len() < 46 + 16 + 4 {
        return None;
    }
    if data[0..4] != HEADER || data[data.len() - 4..] != TAIL {
        return None;
    }
    let mate_id: [u8; 4] = data[14..18].try_into().ok()?;
    let len = u32::from_be_bytes(data[42..46].try_into().ok()?) as usize;
    if 46 + len + 16 + 4 > data.len() {
        return None;
    }
    let cipher = data[46..46 + len].to_vec();
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&data[46 + len..46 + len + 16]);
    Some((mate_id, cipher, tag))
}

pub fn reply_nonce(data: &[u8]) -> Option<[u8; 12]> {
    if data.len() < 38 {
        return None;
    }
    data[26..38].try_into().ok()
}

pub fn reply_add(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 26 {
        return None;
    }
    Some(data[4..26].to_vec())
}

/// Beacon online (porta 6667, type 0x13): decifra col key hardcoded e
/// restituisce il gwId dal JSON. Layout: header(4) add(14) nonce(12) cipher tag(16) crc(4).
pub fn parse_online_report(data: &[u8], key: &[u8; 16]) -> Option<String> {
    if data.len() < 4 + 14 + 12 + 16 + 4 {
        return None;
    }
    if data[0..4] != HEADER {
        return None;
    }
    if data[10..14] != [0, 0, 0, 0x13] {
        return None;
    }
    let add = &data[4..18];
    let nonce = &data[18..30];
    let cipher = &data[30..data.len() - 20];
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&data[data.len() - 20..data.len() - 4]);
    let plain = crypto::gcm_decrypt(key, nonce, add, cipher, &tag)?;
    // l'app rimuove tutti gli spazi dal JSON prima del match
    let text = String::from_utf8(plain).ok()?.replace(char::is_whitespace, "");
    let needle = "\"gwId\":\"";
    let start = text.find(needle)? + needle.len();
    let rest = &text[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_count_is_binary_big_endian_above_nine_devices() {
        let add = build_add(PutType::BusinessRequest, 10, 1, &[1, 2, 3, 4], 320);
        assert_eq!(u32::from_be_bytes(add[18..22].try_into().unwrap()), 10);
    }
}
