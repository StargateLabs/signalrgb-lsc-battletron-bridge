// Reference implementation mirroring LSC Battletron 2.0.17 (from app.asar sources).
// Generates fixtures the Rust bridge must reproduce byte-for-byte.
// Run: node tests/make-fixtures.mjs
import { createCipheriv, createHmac, createHash, randomBytes } from "node:crypto";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const outDir = join(here, "fixtures");

// ---------- original app helpers (parseData.js) ----------
export function getByteDataFromLen(len, data, isEnd = false) {
  data = data.toString();
  const stringLen = len * 2;
  let needLen = stringLen - data.length;
  while (needLen > 0) {
    data = isEnd ? `${data}0` : `0${data}`;
    needLen -= 1;
  }
  return data;
}
export function stringtoHex(str) {
  let val = "";
  for (let i = 0; i < str.length; i++) val += str.charCodeAt(i).toString(16);
  return val;
}
export function sha256Seed(seed, length) {
  const hex = createHash("sha256").update(seed).digest("hex");
  return Buffer.from(hex, "hex").subarray(0, length);
}

// ---------- CRC32 (IEEE reflected), unsigned big-endian hex ----------
const CRC_TABLE = (() => {
  const t = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c;
  }
  return t;
})();
export function crc32UnsignedBeHex(buf) {
  let c = ~0;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  c = ~c >>> 0;
  return c.toString(16).padStart(8, "0");
}
export function crc32Buffer(buf) {
  let c = ~0;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return Buffer.from((~c >>> 0).toString(16).padStart(8, "0"), "hex");
}

// ---------- mate ids (localUdpNetwork/index.js) ----------
export function mainMateIdHex(mainUuidBuffer /*8 bytes*/) {
  // app: getByteDataFromLen(25, stringtoHex(mainUuidHex), true)
  const uuid = getByteDataFromLen(25, stringtoHex(mainUuidBuffer.toString("hex")), true);
  return crc32Buffer(Buffer.from(uuid + "00", "hex")); // 4-byte buffer
}
export function childMateIdHex(devId, mainMateId /*buffer*/) {
  const newDevId = getByteDataFromLen(25, stringtoHex(devId), true);
  return crc32UnsignedBeHex(Buffer.concat([Buffer.from(newDevId + "00", "hex"), mainMateId]));
}

// ---------- frames (localUdpNetwork/index.js) ----------
const HEADER = Buffer.from("00006699", "hex");
const TAIL = Buffer.from("00009966", "hex");
const CONSULT_KEY = Buffer.from("6f36045d84b042e01e29b7c819e37cf7", "hex");
const DATA_LENTH = 42;

function createAd(data, dataLength, typeHex, seqHex, mainMateId) {
  const totalLen = getByteDataFromLen(4, (DATA_LENTH + dataLength).toString(16));
  const frameNum = getByteDataFromLen(4, data.length);
  return Buffer.concat([
    Buffer.from("00" + "00" + seqHex + typeHex, "hex"),
    mainMateId,
    Buffer.from(totalLen + frameNum, "hex"),
  ]);
}
function gcmEncrypt(key, nonce, aad, plaintext) {
  const cipher = createCipheriv("aes-128-gcm", key, nonce);
  cipher.setAAD(aad);
  const data = Buffer.concat([cipher.update(plaintext), cipher.final()]);
  return [data, cipher.getAuthTag()];
}
export function createConsultDataFrame(devIds, mainMateId, seq, nonce, key, tokensAndRandA) {
  let dataLength = (24 + 36) * devIds.length;
  const add = createAd(devIds, dataLength, "00000005", getByteDataFromLen(4, seq.toString(16)), mainMateId);
  const bufferArr = devIds.map((devId) => {
    const { token, randA } = tokensAndRandA[devId];
    const plain = Buffer.concat([Buffer.from("00000000", "hex"), token, randA]);
    const [ct, tag] = gcmEncrypt(key, nonce, add, plain);
    const mateId = childMateIdHex(devId, mainMateId);
    return Buffer.concat([Buffer.from(mateId, "hex"), Buffer.from(getByteDataFromLen(4, ct.length.toString(16)), "hex"), ct, tag]);
  });
  return Buffer.concat([HEADER, add, nonce, ...bufferArr, TAIL]);
}
export function createBusinessFrame(items, mainMateId, seq, nonce, sessionKeys) {
  let dataLength = 0;
  items.forEach((i) => (dataLength += i.value.length / 2 + 24));
  const add = createAd(items, dataLength, "00000010", getByteDataFromLen(4, seq.toString(16)), mainMateId);
  const bufferArr = items.map((item) => {
    const mateId = childMateIdHex(item.devId, mainMateId);
    const key = Buffer.from(sessionKeys[item.devId], "hex");
    const [ct, tag] = gcmEncrypt(key, nonce, add, Buffer.from(item.value, "hex"));
    return Buffer.concat([Buffer.from(mateId, "hex"), Buffer.from(getByteDataFromLen(4, ct.length.toString(16)), "hex"), ct, tag]);
  });
  return Buffer.concat([HEADER, add, nonce, ...bufferArr, TAIL]);
}
// legacy online beacon (connectDevice.js, port 6667)
export function createOnlineBeacon(devId) {
  // legacy layout: header(4) + add(14, with type 0x13 at bytes 10..14) + nonce(12) + cipher + tag(16) + crc(4)
  const nonce = sha256Seed("nonce-beacon", 12);
  const plaintext = Buffer.from(JSON.stringify({ gwId: devId }).replace(/\s+/g, ""));
  const add = Buffer.concat([randomBytes(6), Buffer.from("00000013", "hex"), randomBytes(4)]);
  const key = Buffer.from([108, 30, 200, 226, 187, 155, 181, 154, 181, 11, 13, 175, 100, 155, 65, 10]);
  const [ct, tag] = gcmEncrypt(key, nonce, add, plaintext);
  const crc = Buffer.from("00000000", "hex");
  return Buffer.concat([HEADER, add, nonce, ct, tag, crc]);
}

// ---------- generate fixtures ----------
mkdirSync(outDir, { recursive: true });

const MAC = "A4:C1:38:12:34:56";
const macHexSeed = MAC.replaceAll(":", "");
const mainUuid = sha256Seed(`${macHexSeed}macId`, 8);
const mainMateId = mainMateIdHex(mainUuid);
const devId = "6c8e0d9f3a1b2c4d5e"; // 18 chars
const mateId = childMateIdHex(devId, mainMateId);

const seq = 1;
const nonce = sha256Seed("nonce-fixed", 12);
const token = sha256Seed("token-fixed", 16);
const randA = sha256Seed("randA-fixed", 16);
const sessionKey = sha256Seed("session-fixed", 16);

const consult = createConsultDataFrame([devId], mainMateId, seq, nonce, CONSULT_KEY, {
  [devId]: { token, randA },
});

// dp24: pure red (255,0,0) -> h=0 s=1000 v=1000
const dp24 = Buffer.from(
  getByteDataFromLen(2, (0).toString(16)) +
    getByteDataFromLen(1, parseInt(1000 / 10).toString(16)) +
    getByteDataFromLen(1, parseInt(1000 / 10).toString(16)) +
    "00000100",
  "hex"
);
const business = createBusinessFrame(
  [{ devId, value: dp24.toString("hex") }],
  mainMateId,
  seq + 1,
  nonce,
  { [devId]: sessionKey.toString("hex") }
);

const beacon = createOnlineBeacon(devId);

// expected session derivation (dealData in original): negotiate_key = GCM(token, token[0..12], localKey)[0..16]
const localKey = Buffer.from("00112233445566778899aabbccddeeff", "hex");
const [ngCt] = gcmEncrypt(localKey, token.subarray(0, 12), Buffer.alloc(0), token);
const negotiateKey = ngCt.subarray(0, 16);
function xor(a, b) { return Buffer.from(a.map((x, i) => x ^ b[i])); }
const randB = sha256Seed("randB-fixed", 16);
const randAB = xor(randA, randB);
const [skCt] = gcmEncrypt(negotiateKey, randA.subarray(0, 12), Buffer.alloc(0), randAB);
const sessionKeyDerived = skCt.subarray(0, 16);
const hmacKey = createHmac("sha256", negotiateKey);
const randAHmac = hmacKey.update(randA).digest();
const sessionKeyHmac = createHmac("sha256", negotiateKey).update(sessionKeyDerived).digest();

// dp61 (RGBIC): 4 zone colors (red,green,blue,white), spliceNum=12, stripMap 1..12
function hsv(h, s, v) {
  return Buffer.concat([
    Buffer.from(getByteDataFromLen(2, h.toString(16)), "hex"),
    Buffer.from(getByteDataFromLen(2, s.toString(16)), "hex"),
    Buffer.from(getByteDataFromLen(2, v.toString(16)), "hex"),
  ]);
}
const stripMap = Buffer.from([1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4]);
const zonesHsv = [
  [0, 1000, 1000],
  [120, 1000, 1000],
  [240, 1000, 1000],
  [0, 0, 1000],
];
const dp61 = Buffer.concat([
  Buffer.from("0004", "hex"),
  ...zonesHsv.map((z) => hsv(...z)),
  Buffer.from(getByteDataFromLen(2, stripMap.length.toString(16)), "hex"),
  stripMap,
]);
const business61 = createBusinessFrame(
  [{ devId, value: dp61.toString("hex") }],
  mainMateId,
  seq + 2,
  nonce,
  { [devId]: sessionKey.toString("hex") }
);

const fixtures = {
  mac_hex: macHexSeed,
  dev_id: devId,
  main_uuid_hex: mainUuid.toString("hex"),
  main_mate_id_hex: mainMateId.toString("hex"),
  child_mate_id_hex: mateId,
  crc32_hello: crc32UnsignedBeHex(Buffer.from("hello")),
  seq,
  nonce_hex: nonce.toString("hex"),
  token_hex: token.toString("hex"),
  rand_a_hex: randA.toString("hex"),
  rand_b_hex: randB.toString("hex"),
  local_key_hex: localKey.toString("hex"),
  negotiate_key_hex: negotiateKey.toString("hex"),
  session_key_hex: sessionKey.toString("hex"),
  session_key_derived_hex: sessionKeyDerived.toString("hex"),
  rand_a_hmac_hex: randAHmac.toString("hex"),
  session_key_hmac_hex: sessionKeyHmac.toString("hex"),
  consult_key_hex: CONSULT_KEY.toString("hex"),
  online_key_dec: [108, 30, 200, 226, 187, 155, 181, 154, 181, 11, 13, 175, 100, 155, 65, 10],
  consult_frame_hex: consult.toString("hex"),
  dp24_value_hex: dp24.toString("hex"),
  business_frame_hex: business.toString("hex"),
  dp61_value_hex: dp61.toString("hex"),
  business61_frame_hex: business61.toString("hex"),
  online_beacon_nonce_hex: sha256Seed("nonce-beacon", 12).toString("hex"),
  online_beacon_hex: beacon.toString("hex"),
};
writeFileSync(join(outDir, "protocol.json"), JSON.stringify(fixtures, null, 2));
console.log("fixtures written:", Object.keys(fixtures).length, "keys");
