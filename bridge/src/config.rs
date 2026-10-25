// Configurazione persistente del bridge (%APPDATA%/LscBridge/config.json).
use crate::color::DeviceColorCfg;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceCfg {
    pub dev_id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub rgbic: bool,
    pub local_key: Option<String>,
    #[serde(default)]
    pub color: DeviceColorCfg,
}

fn default_true() -> bool {
    true
}

impl DeviceCfg {
    pub fn new(dev_id: impl Into<String>, name: impl Into<String>, rgbic: bool) -> Self {
        DeviceCfg {
            dev_id: dev_id.into(),
            name: name.into(),
            rgbic,
            local_key: None,
            color: DeviceColorCfg::default(),
        }
    }
}

impl Default for DeviceCfg {
    fn default() -> Self {
        DeviceCfg::new(String::new(), String::new(), true)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// porta TCP del control server (127.0.0.1)
    #[serde(default = "default_port")]
    pub bridge_port: u16,
    /// frame al secondo inviati ai dispositivi
    #[serde(default = "default_fps")]
    pub fps: u32,
    /// regione cloud: "eu" | "us" | "cn"
    #[serde(default = "default_region")]
    pub region: String,
    /// sessione cloud (sid/ecode da QR login)
    #[serde(default)]
    pub session: Option<SessionCfg>,
    /// dispositivi configurati
    #[serde(default)]
    pub devices: Vec<DeviceCfg>,
}

fn default_port() -> u16 {
    21444
}
fn default_fps() -> u32 {
    30
}
fn default_region() -> String {
    "eu".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionCfg {
    pub sid: String,
    pub ecode: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bridge_port: default_port(),
            fps: default_fps(),
            region: default_region(),
            session: None,
            devices: Vec::new(),
        }
    }
}

impl Config {
    pub fn dir() -> PathBuf {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(base).join("LscBridge")
    }

    pub fn path() -> PathBuf {
        Self::dir().join("config.json")
    }

    pub fn load() -> std::io::Result<Self> {
        let p = Self::path();
        match fs::read_to_string(&p) {
            Ok(s) => match serde_json::from_str(&s) {
                Ok(config) => Ok(config),
                Err(error) => {
                    let backup = fs::read_to_string(Self::dir().join("config.json.backup"));
                    if let Some(config) = backup.ok().and_then(|s| serde_json::from_str::<Self>(&s).ok()) {
                        fs::copy(&p, Self::dir().join("config.json.corrupt"))?;
                        eprintln!("lscbridge: configurazione danneggiata preservata; recupero dal backup verificato");
                        Ok(config)
                    } else {
                        Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
                    }
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        fs::create_dir_all(Self::dir())?;
        let json = serde_json::to_string_pretty(self)?;
        // Flush both copies before replacement: a truncated primary must remain recoverable.
        for name in ["config.json.backup", "config.json"] {
            let target = Self::dir().join(name);
            let tmp = target.with_extension("tmp");
            let mut file = fs::File::create(&tmp)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(tmp, target)?;
        }
        Ok(())
    }

    pub fn device(&self, dev_id: &str) -> Option<&DeviceCfg> {
        self.devices.iter().find(|d| d.dev_id == dev_id)
    }

    pub fn device_mut(&mut self, dev_id: &str) -> Option<&mut DeviceCfg> {
        self.devices.iter_mut().find(|d| d.dev_id == dev_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let c = Config::default();
        assert_eq!(c.bridge_port, 21444);
        assert_eq!(c.fps, 30);
        assert_eq!(c.region, "eu");
        assert!(c.devices.is_empty());
    }

    #[test]
    fn roundtrip_json() {
        let mut c = Config::default();
        c.devices.push(DeviceCfg::new("6c8e0d9f3a1b2c4d5e", "Lampada", true));
        let json = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.devices.len(), 1);
        assert_eq!(back.devices[0].name, "Lampada");
        assert!(back.devices[0].rgbic);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let back: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(back.bridge_port, 21444);
        assert_eq!(back.fps, 30);
    }
}
