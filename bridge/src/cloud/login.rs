// Login cloud via QR code (replica di LoginPage.vue: token.create + poll).
use super::TuyaClient;
use serde_json::Value;

pub struct QrLogin {
    pub token: String,
    pub qr_payload: String,
}

fn qr_payload(token: &str) -> String {
    // Formato richiesto dallo scanner SDK Tuya/LSC, non un URI deep link.
    format!("tuyaSmart--qrLogin?token={}", token)
}

/// QR leggibile nel browser, senza servizi esterni o dipendenze aggiuntive.
pub fn qr_html(payload: &str) -> Result<String, String> {
    let mut matrix = qr2term::qr::Qr::from(payload).map_err(|e| e.to_string())?.to_matrix();
    matrix.surround(4, qr2term::render::QrLight);
    let size = matrix.size();
    let mut svg = format!("<svg xmlns='http://www.w3.org/2000/svg' width='420' height='420' viewBox='0 0 {size} {size}' shape-rendering='crispEdges'><rect width='100%' height='100%' fill='white'/><g fill='black'>");
    for (i, pixel) in matrix.pixels().iter().enumerate() {
        if *pixel == qr2term::render::QrDark {
            svg.push_str(&format!("<rect x='{}' y='{}' width='1' height='1'/>", i % size, i / size));
        }
    }
    svg.push_str("</g></svg>");
    use base64::Engine;
    let image = base64::engine::general_purpose::STANDARD.encode(include_bytes!("../../assets/stargatelabs-background.jpg"));
    Ok(include_str!("../../assets/login.html")
        .replace("__BRAND_IMAGE__", &image)
        .replace("__QR_SVG__", &svg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_qr_is_local_and_has_quiet_zone() {
        let html = qr_html(&qr_payload("test")).unwrap();
        assert!(html.contains("<svg"));
        assert!(html.contains("x='4' y='4'"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("src="));
        assert!(html.contains("STARGATE"));
        assert!(!html.contains("__QR_SVG__"));
        assert!(!html.contains("__BRAND_IMAGE__"));
    }

    #[test]
    fn login_qr_matches_tuya_scanner_format() {
        assert_eq!(qr_payload("EUtest"), "tuyaSmart--qrLogin?token=EUtest");
    }
}

impl TuyaClient {
    /// Crea il token QR e ritorna il payload da codificare per lo scanner LSC.
    pub fn qr_login_start(&mut self) -> Result<QrLogin, String> {
        let r = self.call("tuya.m.user.qr.token.create", "1.0", None)?;
        let v = r.get("value").cloned().unwrap_or(Value::Null);
        let token = v.as_str()
            .or_else(|| v.get("token").and_then(|t| t.as_str()))
            .ok_or("token mancante nella risposta")?
            .to_string();
        Ok(QrLogin {
            qr_payload: qr_payload(&token),
            token,
        })
    }

    /// Poll del token: Some((sid, ecode)) quando l'utente ha scannerizzato.
    pub fn qr_login_poll(&mut self, token: &str) -> Result<Option<(String, String)>, String> {
        let r = self.call(
            "tuya.m.user.qr.token.user.get",
            "1.0",
            Some(serde_json::json!({ "token": token })),
        )?;
        let v = r.get("value").cloned().unwrap_or(Value::Null);
        match (v.get("sid").and_then(|s| s.as_str()), v.get("ecode").and_then(|s| s.as_str())) {
            (Some(sid), Some(ecode)) => {
                self.sid = Some(sid.to_string());
                self.ecode = Some(ecode.to_string());
                Ok(Some((sid.to_string(), ecode.to_string())))
            }
            _ => Ok(None),
        }
    }
}
