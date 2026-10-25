pub mod dp;

/// Configurazione colore di un dispositivo (da config.json).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DeviceColorCfg {
    /// true = dispositivo "fantasy color" (dp61, 4 zone), false = dp24 semplice
    pub rgbic: bool,
    /// mappa posizione fisica → zona 1..4 (per dp61)
    pub strip_map: Vec<u8>,
    /// montaggio ruotato 180°
    pub rotated: bool,
    /// zona usata dai dispositivi dp24 (1..4, default 1)
    pub zone_type: u8,
}

impl Default for DeviceColorCfg {
    fn default() -> Self {
        DeviceColorCfg {
            rgbic: false,
            strip_map: vec![1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4],
            rotated: false,
            zone_type: 1,
        }
    }
}

/// Converte i 4 colori LED del canvas di SignalRGB nel valore DP da inviare.
pub fn frame_to_values(leds: &[[u8; 3]; 4], cfg: &DeviceColorCfg) -> Vec<u8> {
    let zones: Vec<(u16, u16, u16)> = leds.iter().map(|l| dp::rgb_to_hsv(*l)).collect();
    if cfg.rgbic {
        dp::dp61_value(&[zones[0], zones[1], zones[2], zones[3]], &cfg.strip_map, cfg.rotated)
    } else {
        let idx = (cfg.zone_type.saturating_sub(1) as usize).min(3);
        dp::dp24_value(zones[idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_device_uses_selected_zone() {
        let cfg = DeviceColorCfg { zone_type: 2, ..Default::default() };
        let leds = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];
        let v = frame_to_values(&leds, &cfg);
        // verde: h=120 → 0078, s=1000/10=100=64, v=64 → 0078646400000100
        assert_eq!(v, hex::decode("0078646400000100").unwrap());
    }

    #[test]
    fn rgbic_device_builds_dp61() {
        let cfg = DeviceColorCfg { rgbic: true, ..Default::default() };
        let leds = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];
        let v = frame_to_values(&leds, &cfg);
        assert_eq!(&v[0..2], &[0x00, 0x04]);
        assert_eq!(v.len(), 2 + 24 + 2 + 12);
    }

    #[test]
    fn default_zone_is_one() {
        assert_eq!(DeviceColorCfg::default().zone_type, 1);
    }
}
