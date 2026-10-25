// Client cloud Tuya OEM — replica di ApiRequest (src/sdk/apiRequest/index.js)
// con firma/secret verificati vs jnimain.dll.
pub mod devices;
pub mod login;
pub mod sign;

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub const APPKEY: &str = "q594qaqdpy89gmvyndtp";
pub const APPSECRET: &str = "xh59e8qykn4sp7jyh7rwaq3ykfwf8e5n";
pub const DEVICE_ID: &str = "022abfd2b476fd3aa679f20df741a88a660c5dddcc6e";
pub const TTID: &str = "sdk_tuya_china@hxm4ffqu73sc8f5twpnm";

pub fn endpoint(region: &str) -> &'static str {
    match region {
        "us" => "a1.tuyaus.com",
        "cn" => "a1.tuyacn.com",
        _ => "a1.tuyaeu.com",
    }
}

fn default_params() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("appRnVersion".into(), "5.27".into());
    m.insert("appVersion".into(), "1.0.0".into());
    m.insert("channel".into(), "oem".into());
    m.insert("clientId".into(), APPKEY.into());
    m.insert("deviceId".into(), DEVICE_ID.into());
    m.insert("et".into(), "0.0.2".into());
    m.insert("lang".into(), "en".into());
    m.insert("os".into(), "Android".into());
    m.insert("osSystem".into(), "11".into());
    m.insert("platform".into(), "Pixel 3".into());
    m.insert("sdkVersion".into(), "3.18.0".into());
    m.insert("timeZoneId".into(), "Asia/Shanghai".into());
    m.insert("ttid".into(), TTID.into());
    m
}

// chiavi che partecipano alla firma (asList in SingKeys.js)
const SIGN_KEYS: &[&str] = &[
    "a", "v", "lat", "lon", "lang", "deviceId", "appVersion", "ttid", "isH5", "h5Token", "os",
    "clientId", "postData", "time", "requestId", "et", "n4h5", "sid", "chKey", "sp",
];

pub struct TuyaClient {
    pub region: String,
    pub sid: Option<String>,
    pub ecode: Option<String>,
}

#[derive(Deserialize)]
struct CloudResponse {
    result: Option<String>,
    #[serde(default)]
    success: Option<bool>,
    #[serde(default, rename = "errorMsg")]
    error_msg: String,
    #[serde(default, rename = "errorCode")]
    error_code: String,
}

impl TuyaClient {
    pub fn new(region: &str) -> Self {
        TuyaClient { region: region.into(), sid: None, ecode: None }
    }

    fn request_id() -> String {
        // uuid v4 casuale
        let mut b: [u8; 16] = rand::random();
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        )
    }

    /// Esegue una chiamata API (a=action, v=version) e ritorna il result decifrato.
    pub fn call(&mut self, action: &str, version: &str, post_data: Option<Value>) -> Result<Value, String> {
        self.call_in_home(action, version, post_data, None)
    }

    pub fn call_in_home(&mut self, action: &str, version: &str, post_data: Option<Value>, gid: Option<&str>) -> Result<Value, String> {
        let rid = Self::request_id();
        let mut params = default_params();
        if let Some(gid) = gid {
            params.insert("gid".into(), gid.into());
        }
        params.insert("a".into(), action.into());
        params.insert("v".into(), version.into());
        params.insert("requestId".into(), rid.clone());
        if let Some(sid) = &self.sid {
            params.insert("sid".into(), sid.clone());
        }
        let post_json = post_data.map(|p| serde_json::to_string(&p).unwrap());
        let secret = sign::post_secret(&rid, self.ecode.as_deref());
        if let Some(pj) = &post_json {
            params.insert("postData".into(), sign::aes_ecb_encrypt_base64(pj, &secret));
        }

        let sign_pairs: Vec<(String, String)> = params
            .iter()
            .filter(|(k, _)| SIGN_KEYS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let s = sign::sign_params(&sign_pairs);
        params.insert("sign".into(), s);

        let body: String = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencode(v)))
            .collect::<Vec<_>>()
            .join("&");

        let host = endpoint(&self.region);
        let resp = https_post(host, body.as_bytes())?;
        let v: Value = serde_json::from_str(&resp)
            .map_err(|e| format!("risposta non JSON: {} ({})", e, &resp[..resp.len().min(120)]))?;
        // verifica firma risposta: MD5(result=<result>||t=<t>||<secret>) come l'app
        if let (Some(result), Some(t), Some(sign)) = (
            v.get("result").and_then(|x| x.as_str()),
            v.get("t").and_then(|x| x.as_str()),
            v.get("sign").and_then(|x| x.as_str()),
        ) {
            let expect = sign::md5_hex(&format!("result={}||t={}||{}", result, t, secret));
            if expect != sign {
                return Err("firma della risposta cloud non valida".into());
            }
        }
        let fields = v.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
        let parsed: CloudResponse = serde_json::from_value(v).map_err(|e| format!("campi risposta: {}", e))?;
        if parsed.success == Some(false) || parsed.result.is_none() {
            let msg = if parsed.error_msg.is_empty() {
                format!("errore cloud senza errorMsg (campi: {})", fields.join(", "))
            } else {
                parsed.error_msg.clone()
            };
            return Err(if parsed.error_code.is_empty() {
                msg
            } else {
                format!("{} ({})", msg, parsed.error_code)
            });
        }
        let cipher = parsed.result.ok_or("risposta senza result")?;
        let plain = sign::aes_ecb_decrypt_base64(&cipher, &secret).ok_or("decrypt result fallita")?;
        let value: Value = serde_json::from_str(&plain).map_err(|e| format!("result non JSON: {}", e))?;
        if value.get("success").and_then(Value::as_bool) == Some(false) {
            return Err(format!("{} ({})",
                value.get("errorMsg").and_then(Value::as_str).unwrap_or("errore cloud"),
                value.get("errorCode").and_then(Value::as_str).unwrap_or("senza codice")));
        }
        let result = value.get("result").cloned().unwrap_or(value);
        Ok(json!({ "value": result }))
    }
}

#[cfg(test)]
mod request_tests {
    use super::*;

    #[test]
    fn request_id_is_uuid_v4() {
        let id = TuyaClient::request_id();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!(matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
        assert_eq!(id.split('-').map(str::len).collect::<Vec<_>>(), vec![8, 4, 4, 4, 12]);
    }

    #[test]
    fn encrypted_cloud_envelope_does_not_require_outer_success() {
        let response: CloudResponse = serde_json::from_value(json!({
            "result": "encrypted", "sign": "signature", "t": 123
        })).unwrap();
        assert_eq!(response.success, None);
        assert_eq!(response.result.as_deref(), Some("encrypted"));
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// POST HTTPS minimale via native-tls (nessuna dipendenza tokio/ring).
fn https_post(host: &str, body: &[u8]) -> Result<String, String> {
    use native_tls::TlsConnector;
    let connector = TlsConnector::new().map_err(|e| e.to_string())?;
    let stream = TcpStream::connect((host, 443)).map_err(|e| format!("connect {}: {}", host, e))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|e| e.to_string())?;
    let mut tls = connector
        .connect(host, stream)
        .map_err(|e| format!("tls {}: {}", host, e))?;
    let req = format!(
        "POST /api.json HTTP/1.1\r\nHost: {}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        host,
        body.len()
    );
    tls.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    tls.write_all(body).map_err(|e| e.to_string())?;
    tls.flush().ok();
    let mut raw = Vec::new();
    tls.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw).to_string();
    let header_end = text.find("\r\n\r\n").ok_or("risposta HTTP senza header")?;
    let status_line = text.lines().next().unwrap_or("");
    if !status_line.contains("200") {
        return Err(format!("HTTP non-200: {}", status_line));
    }
    let body = text[header_end + 4..].to_string();
    // se chunked, de-chunka
    if text.to_lowercase().contains("transfer-encoding: chunked") {
        let mut out = String::new();
        let mut rest: &str = &body;
        loop {
            let eol = rest.find("\r\n").ok_or("chunk malformato")?;
            let size = usize::from_str_radix(rest[..eol].trim(), 16).map_err(|e| e.to_string())?;
            if size == 0 {
                break;
            }
            out.push_str(&rest[eol + 2..eol + 2 + size]);
            rest = &rest[eol + 2 + size + 2..];
        }
        return Ok(out);
    }
    Ok(body)
}
