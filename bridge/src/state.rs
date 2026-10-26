// Stato condiviso tra control server, loop UDP e cloud.
use crate::config::Config;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Ultimo frame colore ricevuto per dispositivo (4 zone × RGB).
pub type FrameMap = HashMap<String, [[u8; 3]; 4]>;

#[derive(Default)]
pub struct Inner {
    pub config: Config,
    pub latest: FrameMap,
    /// Spegnimenti da trasmettere prima di rimuovere l'ultimo frame.
    pub release_after_send: HashSet<String>,
    pub frame_received_at: HashMap<String, Instant>,
    pub black_since: HashMap<String, Instant>,
    pub pending_power: HashMap<String, bool>,
    pub power_requested: HashMap<String, bool>,
    pub power_confirmed: HashMap<String, bool>,
    pub renegotiate: HashSet<String>,
    /// dispositivi risultati online (devId)
    pub online: Vec<String>,
    pub running_ok: bool,
}

#[derive(Clone)]
pub struct SharedState(pub Arc<RwLock<Inner>>);

impl Default for SharedState {
    fn default() -> Self {
        SharedState::new(Config::default())
    }
}

impl SharedState {
    pub fn new(config: Config) -> Self {
        SharedState(Arc::new(RwLock::new(Inner {
            config,
            latest: HashMap::new(),
            release_after_send: HashSet::new(),
            frame_received_at: HashMap::new(),
            black_since: HashMap::new(),
            pending_power: HashMap::new(),
            power_requested: HashMap::new(),
            power_confirmed: HashMap::new(),
            renegotiate: HashSet::new(),
            online: Vec::new(),
            running_ok: true,
        })))
    }

    pub fn read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        self.0.read().unwrap()
    }
    pub fn write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        self.0.write().unwrap()
    }
}

impl Inner {
    /// Stop LAN traffic before cloud OFF: even black LAN packets can wake a light.
    pub fn prepare_power_off(&mut self, id: &str) -> bool {
        if self.power_requested.get(id) != Some(&false) { return false; }
        if self.latest.get(id).is_some_and(|frame| *frame != [[0;3];4]) { return false; }
        self.latest.remove(id);
        self.release_after_send.remove(id);
        true
    }

    pub fn replace_config(&mut self, config: Config) {
        for device in &config.devices {
            if self.config.device(&device.dev_id)
                .is_some_and(|old| old.local_key != device.local_key) {
                self.renegotiate.insert(device.dev_id.clone());
            }
        }
        self.config = config;
    }

    pub fn request_power(&mut self, id: &str, on: bool) {
        // Lo stream LAN riaccende il dispositivo; un ON cloud tardivo ripristina
        // il vecchio colore. Annulla gli OFF accodati quando riprende un colore.
        if on {
            self.power_confirmed.remove(id);
            self.power_requested.insert(id.into(), true);
            self.pending_power.remove(id);
            return;
        }
        if self.power_requested.get(id) != Some(&on) {
            self.power_requested.insert(id.into(), on);
            self.pending_power.insert(id.into(), on);
        }
    }
}

/// Vista dispositivo per il plugin (LIST).
#[derive(Serialize)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub rgbic: bool,
    pub pixels: usize,
    pub online: bool,
    pub leds: Option<[[u8; 3]; 4]>,
}

impl SharedState {
    pub fn device_views(&self) -> Vec<DeviceView> {
        let g = self.read();
        g.config
            .devices
            .iter()
            .map(|d| DeviceView {
                id: d.dev_id.clone(),
                name: d.name.clone(),
                rgbic: d.rgbic,
                pixels: 4,
                online: g.online.contains(&d.dev_id),
                leds: g.latest.get(&d.dev_id).copied(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DeviceCfg;

    #[test]
    fn device_views_lists_configured_devices() {
        let s = SharedState::default();
        s.write().config.devices.push(DeviceCfg::new("abc", "Lampada", true));
        let views = s.device_views();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].id, "abc");
        assert!(views[0].rgbic);
    }

    #[test]
    fn power_changes_coalesce_and_do_not_repeat_per_frame() {
        let mut state = Inner::default();
        state.request_power("dev", true);
        state.pending_power.clear();
        state.request_power("dev", true);
        assert!(state.pending_power.is_empty());
        state.request_power("dev", false);
        assert_eq!(state.pending_power.get("dev"), Some(&false));
        state.request_power("dev", true);
        assert!(state.pending_power.is_empty(), "il colore cancella OFF, non accoda ON cloud");
    }

    #[test]
    fn config_reload_marks_only_devices_with_changed_keys() {
        let mut state = Inner::default();
        let mut a = DeviceCfg::new("a", "A", true);
        a.local_key = Some("old".into());
        state.config.devices = vec![a, DeviceCfg::new("b", "B", true)];
        let mut updated = state.config.clone();
        updated.devices[0].local_key = Some("new".into());
        updated.devices.push(DeviceCfg::new("c", "C", false));
        state.replace_config(updated);
        assert_eq!(state.renegotiate, HashSet::from(["a".to_string()]));
        assert_eq!(state.config.devices.len(), 3);
    }
}
