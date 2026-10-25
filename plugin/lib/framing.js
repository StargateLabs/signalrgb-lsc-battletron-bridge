// Funzioni pure del plugin: framing del protocollo col bridge,
// normalizzazione colori. Nessuna dipendenza da SignalRGB (testabile in Node).
//
// Protocollo: [u32 big-endian len][json UTF-8]

export function protocolEncode(obj) {
  const payload = JSON.stringify(obj).replace(/[\u007f-\uffff]/g, c => "\\u" + ("0000" + c.charCodeAt(0).toString(16)).slice(-4));
  const bytes = [];
  for (let i = 0; i < payload.length; i++) bytes.push(payload.charCodeAt(i) & 0xff);
  const header = [
    (bytes.length >>> 24) & 0xff,
    (bytes.length >>> 16) & 0xff,
    (bytes.length >>> 8) & 0xff,
    bytes.length & 0xff,
  ];
  return header.concat(bytes);
}

export function protocolDecode(byteArray) {
  if (!byteArray || byteArray.length < 4) throw new Error("frame troppo corto");
  const len =
    ((byteArray[0] << 24) | (byteArray[1] << 16) | (byteArray[2] << 8) | byteArray[3]) >>> 0;
  if (byteArray.length < 4 + len) throw new Error("frame incompleto");
  let text = "";
  for (let i = 4; i < 4 + len; i++) text += String.fromCharCode(byteArray[i]);
  return JSON.parse(decodeURIComponent(escape(text)));
}

// TCP può spezzare un frame o consegnarne più di uno nello stesso evento.
export function createProtocolReader() {
  let pending = [];
  return function (chunk) {
    if (chunk && !Array.isArray(chunk) && typeof chunk === "object") {
      // data è testo UTF-8: corrompe il prefisso binario. buffer conserva i byte.
      if (chunk.buffer !== undefined && chunk.length === undefined) chunk = new Uint8Array(chunk.buffer);
      else if (chunk.data !== undefined) chunk = chunk.data;
      else if (chunk.response !== undefined) chunk = chunk.response;
    }
    if (typeof chunk === "string") chunk = Array.from(chunk, c => c.charCodeAt(0) & 255);
    pending = pending.concat(Array.from(chunk));
    const messages = [];
    while (pending.length >= 4) {
      const len = ((pending[0] << 24) | (pending[1] << 16) | (pending[2] << 8) | pending[3]) >>> 0;
      if (len > 65536) throw new Error("frame troppo grande");
      if (pending.length < len + 4) break;
      messages.push(protocolDecode(pending.slice(0, len + 4)));
      pending = pending.slice(len + 4);
    }
    return messages;
  };
}

export function hexToRgb(hexColor) {
  const result = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hexColor || "#000000");
  if (!result) return [0, 0, 0];
  return [parseInt(result[1], 16), parseInt(result[2], 16), parseInt(result[3], 16)];
}

// device.color può restituire [r,g,b] oppure {r,g,b} a seconda delle versioni
export function normalizeColor(c) {
  if (c && typeof c.length === "number" && c.length >= 3) return [c[0] | 0, c[1] | 0, c[2] | 0];
  if (c && typeof c === "object") return [c.r | 0, c.g | 0, c.b | 0];
  return [0, 0, 0];
}
