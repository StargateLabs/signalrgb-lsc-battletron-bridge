// Lista dispositivi dal cloud (replica di HomePage.getDevList) — include
// la localKey necessaria alla negoziazione sulla LAN.
use super::TuyaClient;
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct CloudDevice {
    pub dev_id: String,
    pub name: String,
    pub product_id: String,
    pub local_key: String,
    pub rgbic: bool,
}

impl TuyaClient {
    /// Il nero LAN azzera i colori; DP20 disattiva realmente l'alimentazione LED.
    pub fn set_light_power(&mut self, gid: &str, dev_id: &str, on: bool) -> Result<(), String> {
        let result = self.call_in_home("tuya.m.device.dp.publish", "1.0", Some(serde_json::json!({
            "devId": dev_id, "gwId": dev_id, "dps": {"20": on}
        })), Some(gid))?;
        if result["value"].as_bool() == Some(false) {
            return Err("comando alimentazione rifiutato dal cloud".into());
        }
        // L'ACK del cloud non prova che DP20 sia già applicato sul dispositivo.
        for _ in 0..4 {
            let reported = self.call_in_home("tuya.m.device.dp.get", "1.0",
                Some(serde_json::json!({"devId":dev_id})), Some(gid))?;
            let value = &reported["value"];
            let dps = value.get("dps").unwrap_or(value);
            if dps["20"].as_bool() == Some(on) { return Ok(()); }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        Err("DP20 non ancora confermato dal dispositivo".into())
    }

    /// Primo gid della lista home.
    pub fn first_home_id(&mut self) -> Result<String, String> {
        let r = self.call("tuya.m.location.list", "4.0", None)?;
        let homes = r.get("value").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        homes
            .first()
            .and_then(|h| h.get("gid"))
            .and_then(home_id)
            .ok_or_else(|| "nessuna casa trovata nell'account".into())
    }

    /// Dispositivi della casa (filtrati sui PID supportati se dato).
    pub fn devices(&mut self, gid: &str) -> Result<Vec<CloudDevice>, String> {
        let r = self.call_in_home(
            "tuya.m.my.group.device.list",
            "2.0",
            None,
            Some(gid),
        )?;
        let mut out = Vec::new();
        if let Some(arr) = r.get("value").and_then(|v| v.as_array()) {
            for d in arr {
                let dev_id = d.get("devId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if dev_id.is_empty() {
                    continue;
                }
                let local_key = d.get("localKey").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let name = d.get("name").and_then(|v| v.as_str()).unwrap_or(&dev_id).to_string();
                let product_id = d.get("productId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                // dp 61 (paint_colour_data) ⇒ dispositivo "fantasy color" (dp61, 4 zone)
                let rgbic = uses_rgbic_stream(&product_id, d);
                out.push(CloudDevice { dev_id, name, product_id, local_key, rgbic });
            }
        }
        Ok(out)
    }
}

fn uses_rgbic_stream(product_id: &str, device: &Value) -> bool {
    // LSC 2.0.17, store/mutations/index.js blackList: la Ball Light espone DP61
    // per le scene ma il suo streaming Chroma usa il payload semplice dp24.
    product_id != "yrg6a649fkozp1hw" && has_dp(device, 61)
}

fn home_id(gid: &Value) -> Option<String> {
    match gid {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_numeric_and_string_home_ids() {
        assert_eq!(home_id(&serde_json::json!(123456)), Some("123456".into()));
        assert_eq!(home_id(&serde_json::json!("123456")), Some("123456".into()));
        assert_eq!(home_id(&Value::Null), None);
    }

    #[test]
    fn ball_light_matches_vendor_simple_stream_exception() {
        let device = serde_json::json!({"dataPointInfo":{"dps":{"61":"scene data"}}});
        assert!(!uses_rgbic_stream("yrg6a649fkozp1hw", &device));
        assert!(uses_rgbic_stream("hy57lewrecoee6hv", &device));
        assert!(!uses_rgbic_stream("other", &serde_json::json!({})));
    }
}

fn has_dp(device: &Value, dp: i64) -> bool {
    device
        .get("dataPointInfo")
        .and_then(|d| d.get("dps"))
        .map(|d| d.get(&dp.to_string()).is_some())
        .unwrap_or(false)
}
