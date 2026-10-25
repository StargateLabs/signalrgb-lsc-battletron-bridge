use crate::config::DeviceCfg;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct Entry {
    local_key_fingerprint: String,
    session: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct Cache {
    version: u8,
    main_mate_id: [u8; 4],
    devices: HashMap<String, Entry>,
}

fn fingerprint(device: &DeviceCfg) -> Option<String> {
    device.local_key.as_deref().filter(|key| !key.is_empty())
        .map(|key| hex::encode(Sha256::digest(key.as_bytes())))
}

pub fn load(path: &Path, main: [u8; 4], devices: &[DeviceCfg]) -> io::Result<HashMap<String, Vec<u8>>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error),
    };
    let cache: Cache = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "cache sessioni non valida"))?;
    if cache.version != 1 || cache.main_mate_id != main {
        return Ok(HashMap::new());
    }
    Ok(devices.iter().filter_map(|device| {
        let entry = cache.devices.get(&device.dev_id)?;
        if entry.session.len() != 16 || fingerprint(device)? != entry.local_key_fingerprint {
            return None;
        }
        Some((device.dev_id.clone(), entry.session.clone()))
    }).collect())
}

pub fn save(path: &Path, main: [u8; 4], devices: &[DeviceCfg], sessions: &HashMap<String, Vec<u8>>) -> io::Result<()> {
    let entries = devices.iter().filter_map(|device| {
        let session = sessions.get(&device.dev_id)?;
        if session.len() != 16 { return None; }
        Some((device.dev_id.clone(), Entry {
            local_key_fingerprint: fingerprint(device)?,
            session: session.clone(),
        }))
    }).collect();
    let cache = Cache { version: 1, main_mate_id: main, devices: entries };
    let bytes = serde_json::to_vec(&cache)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "cache sessioni non serializzabile"))?;
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPath(std::path::PathBuf);
    impl TestPath {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("lsc-session-cache-{}-{}", std::process::id(), rand::random::<u64>())).join("lan-sessions.json"))
        }
    }
    impl Drop for TestPath {
        fn drop(&mut self) { let _ = fs::remove_dir_all(self.0.parent().unwrap()); }
    }
    fn device(key: &str) -> DeviceCfg {
        let mut device = DeviceCfg::new("dev", "Test", true);
        device.local_key = Some(key.into());
        device
    }

    #[test]
    fn cache_accepts_only_same_host_device_and_local_key() {
        let path = TestPath::new();
        let devices = vec![device("abcdefghijklmnop")];
        let sessions = HashMap::from([("dev".into(), vec![7; 16])]);
        save(&path.0, [1, 2, 3, 4], &devices, &sessions).unwrap();
        assert_eq!(load(&path.0, [1, 2, 3, 4], &devices).unwrap(), sessions);
        assert!(load(&path.0, [0; 4], &devices).unwrap().is_empty());
        assert!(load(&path.0, [1, 2, 3, 4], &[device("changed")]).unwrap().is_empty());
        assert!(load(&path.0, [1, 2, 3, 4], &[]).unwrap().is_empty());
        assert!(!String::from_utf8(fs::read(&path.0).unwrap()).unwrap().contains("abcdefghijklmnop"));
    }

    #[test]
    fn cache_replacement_removes_invalidated_sessions_and_rejects_bad_lengths() {
        let path = TestPath::new();
        let devices = vec![device("abcdefghijklmnop")];
        save(&path.0, [1; 4], &devices, &HashMap::from([("dev".into(), vec![7; 16])])).unwrap();
        save(&path.0, [1; 4], &devices, &HashMap::new()).unwrap();
        assert!(load(&path.0, [1; 4], &devices).unwrap().is_empty());
        save(&path.0, [1; 4], &devices, &HashMap::from([("dev".into(), vec![7; 15])])).unwrap();
        assert!(load(&path.0, [1; 4], &devices).unwrap().is_empty());
        assert!(!path.0.with_extension("json.tmp").exists());
    }

    #[test]
    fn missing_cache_is_empty_and_corrupt_cache_is_not_loaded() {
        let path = TestPath::new();
        assert!(load(&path.0, [1; 4], &[]).unwrap().is_empty());
        fs::create_dir_all(path.0.parent().unwrap()).unwrap();
        fs::write(&path.0, b"broken").unwrap();
        assert_eq!(load(&path.0, [1; 4], &[]).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
