// Runtime UDP: discovery broadcast (40001), negoziazione consult,
// invio business data e watcher online (6667). Replica di UdpNetwork +
// BroadcastUdp + connectDevice dell'app originale (v2.0.17).
use crate::color;
use crate::config::DeviceCfg;
use crate::proto::frame::{self, PutType, HEADER, TAIL};
use crate::proto::{crypto, mate};
use crate::state::SharedState;
use crate::util;
use if_addrs::get_if_addrs;
use rand::RngCore;
use std::collections::HashMap;
use std::net::{IpAddr, UdpSocket};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

#[path = "session_cache.rs"]
mod session_cache;

pub const CONSULT_KEY_HEX: &str = "6f36045d84b042e01e29b7c819e37cf7";
pub const BROADCAST_PORT: u16 = 40001;
pub const ONLINE_PORT: u16 = 6667;
pub const ONLINE_KEY: [u8; 16] = [
    108, 30, 200, 226, 187, 155, 181, 154, 181, 11, 13, 175, 100, 155, 65, 10,
];
const CONSULT_RETRY: Duration = Duration::from_millis(2000);
const CONSULT_MAX_TRIES: u32 = 4; // come l'originale: ~10s poi stop
const CONSULT_BACKOFF_AFTER: Duration = Duration::from_secs(30);
const CONSULT_BATCH: usize = 5; // l'originale raggruppa 5 dispositivi per frame
const ONLINE_TIMEOUT: Duration = Duration::from_secs(20);

pub enum RtMsg {
    Reply(Vec<u8>),
    Sent(Vec<u8>, Option<IpAddr>, Instant),
    Online(String, IpAddr),
}

struct Negotiation {
    token: Vec<u8>,
    rand_a: Vec<u8>,
    tries: u32,
}

/// Store sessioni: mate_id → session_key.
#[derive(Default)]
pub struct SessionStore {
    map: HashMap<[u8; 4], Vec<u8>>,
    dev_to_mate: HashMap<String, [u8; 4]>,
}

impl SessionStore {
    pub fn insert(&mut self, mate_id: [u8; 4], key: Vec<u8>) {
        self.map.insert(mate_id, key);
    }
    pub fn has(&self, mate_id: &[u8; 4]) -> bool {
        self.map.contains_key(mate_id)
    }
    pub fn get(&self, mate_id: &[u8; 4]) -> Option<&Vec<u8>> {
        self.map.get(mate_id)
    }
    pub fn mate_of(&self, dev_id: &str, main: &[u8; 4]) -> [u8; 4] {
        self.dev_to_mate
            .get(dev_id)
            .copied()
            .unwrap_or_else(|| mate::child_mate_id(dev_id, main))
    }
    pub fn register_dev(&mut self, dev_id: &str, mate_id: [u8; 4]) {
        self.dev_to_mate.insert(dev_id.to_string(), mate_id);
    }
    pub fn retain_valid_devices(&mut self, dev_ids: &[String], main: &[u8; 4]) {
        self.map
            .retain(|m, _| dev_ids.iter().any(|d| mate::child_mate_id(d, main) == *m));
    }
}

pub struct UdpRuntime {
    state: SharedState,
    out40001: Vec<Sender<(Instant, Vec<u8>, Option<IpAddr>)>>,
    #[allow(dead_code)]
    out6667: Vec<Sender<Vec<u8>>>,
    rx: Receiver<RtMsg>,
    pub main_mate_id: [u8; 4],
    seq: u32,
    sessions: SessionStore,
    cache_path: Option<std::path::PathBuf>,
    negotiations: HashMap<String, Negotiation>,
    consult_key: Vec<u8>,
    last_consult: HashMap<String, Instant>,
    last_sent: HashMap<String, Vec<u8>>,
    last_sent_at: HashMap<String, Instant>,
    last_online: HashMap<String, Instant>,
    peers: HashMap<String, IpAddr>,
    last_route_count: Option<usize>,
    frame_period: Duration,
    pub socket_count: usize,
}

/// Interfacce IPv4 non-loopback col relativo indirizzo di broadcast.
pub fn interfaces() -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(ifaces) = get_if_addrs() {
        for i in ifaces {
            if let if_addrs::IfAddr::V4(v4) = &i.addr {
                // APIPA non ha una rete configurata: evita copie dei comandi LAN
                // sulle schede Wi-Fi/Bluetooth scollegate.
                if i.is_loopback() || v4.ip.is_link_local() {
                    continue;
                }
                let ip = v4.ip.octets();
                let mask = v4.netmask.octets();
                let bc: Vec<String> =
                    ip.iter().zip(mask.iter()).map(|(a, m)| (a | !m).to_string()).collect();
                out.push((v4.ip.to_string(), bc.join(".")));
            }
        }
    }
    out
}

/// localKey (stringa dal cloud, tipicamente 16 char) → 16 byte ASCII
/// con padding a destra di '0'. Replica esatta di
/// getBuffer(getByteDataFromLen(16, stringtoHex(localKey), true)) nell'app.
pub fn local_key_bytes(local_key: &str) -> Vec<u8> {
    let hex = crate::util::string_to_hex(local_key);
    let padded = crate::util::pad_hex(16, &hex, true);
    crate::util::hex_decode(&padded).unwrap()
}

impl UdpRuntime {
    pub fn new(state: SharedState, fps: u32) -> Self {
        let main_uuid = util::mac_uuid_hex(&first_mac_hex());
        let main_mate_id = mate::main_mate_id(&main_uuid);
        let (tx, rx) = channel();
        let mut out40001 = Vec::new();
        let mut out6667 = Vec::new();
        let mut socket_count = 0usize;

        for (addr, bcast) in interfaces() {
            // 40001: send+recv verso il broadcast dell'interfaccia
            match Self::bind(&addr, BROADCAST_PORT) {
                Ok(sock) => {
                    let _ = sock.set_broadcast(true);
                    let _ = sock.set_read_timeout(Some(Duration::from_millis(2)));
                    let (tx_out, rx_out) = channel();
                    let tx_in = tx.clone();
                    thread::spawn(move || socket_thread(sock, tx_in, rx_out, false, bcast.clone()));
                    out40001.push(tx_out);
                    socket_count += 1;
                }
                Err(e) => {
                    eprintln!(
                        "lscbridge: bind UDP {}:{} fallito: {} (l'app LSC originale è aperta? \
                         chiudila o riavvia questo bridge)",
                        addr, BROADCAST_PORT, e
                    );
                    continue;
                }
            }
            // 6667: solo ricezione (beacon online)
            match UdpSocket::bind((addr.as_str(), ONLINE_PORT)) {
                Ok(sock) => {
                    let _ = sock.set_read_timeout(Some(Duration::from_millis(50)));
                    let tx_in = tx.clone();
                    thread::spawn(move || recv_only_thread(sock, tx_in));
                    socket_count += 1;
                }
                Err(_) => continue,
            }
        }

        #[cfg(not(test))]
        let cache_path = Some(crate::config::Config::dir().join("lan-sessions.json"));
        #[cfg(test)]
        let cache_path = None;
        let mut runtime = UdpRuntime {
            state,
            out40001,
            out6667,
            rx,
            main_mate_id,
            seq: 1,
            sessions: SessionStore::default(),
            cache_path,
            negotiations: HashMap::new(),
            consult_key: util::hex_decode(CONSULT_KEY_HEX).unwrap(),
            last_consult: HashMap::new(),
            last_sent: HashMap::new(),
            last_sent_at: HashMap::new(),
            last_online: HashMap::new(),
            peers: HashMap::new(),
            last_route_count: None,
            frame_period: Duration::from_millis(1000 / fps.max(1) as u64),
            socket_count,
        };
        runtime.restore_sessions();
        runtime
    }

    fn restore_sessions(&mut self) {
        let Some(path) = &self.cache_path else { return; };
        let devices = self.state.read().config.devices.clone();
        match session_cache::load(path, self.main_mate_id, &devices) {
            Ok(cached) => for (id, key) in cached {
                let mate_id = mate::child_mate_id(&id, &self.main_mate_id);
                self.sessions.insert(mate_id, key);
                self.sessions.register_dev(&id, mate_id);
            },
            Err(_) => eprintln!("lscbridge: cache sessioni LAN non disponibile; nuova negoziazione"),
        }
    }

    fn save_sessions(&self) {
        let Some(path) = &self.cache_path else { return; };
        let state = self.state.read();
        // Un reload puo avere gia cambiato localKey prima del prossimo tick UDP.
        let devices: Vec<DeviceCfg> = state.config.devices.iter()
            .filter(|device| !state.renegotiate.contains(&device.dev_id)).cloned().collect();
        drop(state);
        let sessions = devices.iter().filter_map(|device| {
            let mate_id = self.sessions.mate_of(&device.dev_id, &self.main_mate_id);
            self.sessions.get(&mate_id).map(|key| (device.dev_id.clone(), key.clone()))
        }).collect();
        if session_cache::save(path, self.main_mate_id, &devices, &sessions).is_err() {
            eprintln!("lscbridge: salvataggio cache sessioni LAN non riuscito");
        }
    }

    fn bind(addr: &str, port: u16) -> std::io::Result<UdpSocket> {
        use socket2::{Domain, Protocol, Socket, Type};
        // socket2 permette SO_REUSEADDR anche su Windows: coesistenza con
        // l'app LSC ufficiale che bind sulla stessa porta.
        let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        sock.set_reuse_address(true)?;
        let sa: std::net::SocketAddr = format!("{}:{}", addr, port)
            .parse()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "indirizzo non valido"))?;
        sock.bind(&sa.into())?;
        Ok(sock.into())
    }

    fn broadcast(&self, data: &[u8]) {
        for s in &self.out40001 {
            let _ = s.send((Instant::now(), data.to_vec(), None));
        }
    }

    /// Gestisce una risposta consult (formato reply device).
    pub fn handle_reply(&mut self, data: &[u8]) {
        let (mate_id, cipher, tag) = match frame::parse_reply(data) {
            Some(v) => v,
            None => return,
        };
        let add = match frame::reply_add(data) {
            Some(a) => a,
            None => return,
        };
        let nonce = match frame::reply_nonce(data) {
            Some(n) => n.to_vec(),
            None => return,
        };
        let dev_id = {
            let st = self.state.read();
            match st
                .config
                .devices
                .iter()
                .find(|d| mate::child_mate_id(&d.dev_id, &self.main_mate_id) == mate_id)
                .map(|d| d.dev_id.clone())
            {
                Some(i) => i,
                None => return,
            }
        };
        let (token, rand_a) = match self.negotiations.get(&dev_id) {
            Some(n) => (n.token.clone(), n.rand_a.clone()),
            None => return,
        };
        let local_key = {
            let st = self.state.read();
            match st.config.device(&dev_id).and_then(|d| d.local_key.clone()) {
                Some(k) if !k.is_empty() => local_key_bytes(&k),
                _ => return,
            }
        };
        let negotiate_key = crypto::negotiate_key(&local_key, &token);
        let plain = match crypto::gcm_decrypt(&negotiate_key, &nonce, &add, &cipher, tag.as_slice()) {
            Some(p) => p,
            None => return,
        };
        if plain.len() < 80 {
            return;
        }
        let rand_b = &plain[0..16];
        let rand_a_hmac = &plain[16..48];
        let session_hmac = &plain[48..80];
        if crypto::hmac_sha256(&negotiate_key, &rand_a).as_slice() != rand_a_hmac {
            return;
        }
        let rand_ab = util::xor(&rand_a, rand_b);
        let session = crypto::session_key(&negotiate_key, &rand_a, &rand_ab);
        if crypto::hmac_sha256(&negotiate_key, &session).as_slice() != session_hmac {
            return;
        }
        self.sessions.insert(mate_id, session);
        self.sessions.register_dev(&dev_id, mate_id);
        self.save_sessions();
        self.negotiations.remove(&dev_id);
        self.last_online.insert(dev_id.clone(), Instant::now());
        eprintln!("lscbridge: sessione LAN autenticata per {}", dev_id);
        let mut st = self.state.write();
        if !st.online.contains(&dev_id) {
            st.online.push(dev_id);
        }
    }

    /// Consult raggruppato (max 5 per frame, come l'originale) con stop dopo
    /// 4 tentativi per ciclo e backoff di 30s.
    pub fn consult_missing(&mut self) {
        // Un nuovo abbinamento può cambiare la localKey mantenendo lo stesso ID.
        let changed: Vec<String> = self.state.write().renegotiate.drain().collect();
        let cache_invalidated = !changed.is_empty();
        for id in changed {
            let mate_id = self.sessions.mate_of(&id, &self.main_mate_id);
            self.sessions.map.remove(&mate_id);
            self.negotiations.remove(&id);
            self.last_consult.remove(&id);
            self.last_online.remove(&id);
            self.last_sent.remove(&id);
            self.last_sent_at.remove(&id);
            let mut state = self.state.write();
            state.online.retain(|online| online != &id);
            state.power_requested.remove(&id);
        }
        let devices: Vec<DeviceCfg> = self.state.read().config.devices.clone();
        let dev_ids: Vec<String> = devices.iter().map(|d| d.dev_id.clone()).collect();
        let previous_sessions = self.sessions.map.len();
        self.sessions.retain_valid_devices(&dev_ids, &self.main_mate_id);
        if cache_invalidated || self.sessions.map.len() != previous_sessions {
            self.save_sessions();
        }
        // scadenza online (20s senza beacon ⇒ offline)
        let now = Instant::now();
        let expired: Vec<String> = self
            .last_online
            .iter()
            .filter(|(_, t)| now.duration_since(**t) > ONLINE_TIMEOUT)
            .map(|(d, _)| d.clone())
            .collect();
        if !expired.is_empty() {
            let mut st = self.state.write();
            for d in &expired {
                st.online.retain(|x| x != d);
                // Come l'originale: la perdita del beacon non invalida la sessione.
                // Le chiavi cambiate vengono invalidate dal reload della configurazione.
                self.last_sent.remove(d);
                self.last_sent_at.remove(d);
                self.last_online.remove(d);
            }
        }

        let mut batch: Vec<(String, [u8; 4], Negotiation)> = Vec::new();
        for d in devices {
            let mate_id = self.sessions.mate_of(&d.dev_id, &self.main_mate_id);
            if self.sessions.has(&mate_id) {
                continue;
            }
            let now = Instant::now();
            let entry = self.negotiations.entry(d.dev_id.clone()).or_insert(Negotiation {
                token: rand_bytes(16),
                rand_a: rand_bytes(16),
                tries: 0,
            });
            if entry.tries >= CONSULT_MAX_TRIES {
                // backoff: ritenta dopo CONSULT_BACKOFF_AFTER dall'ultimo tentativo
                let last = self.last_consult.get(&d.dev_id).copied().unwrap_or(now - CONSULT_BACKOFF_AFTER);
                if now.duration_since(last) < CONSULT_BACKOFF_AFTER {
                    continue;
                }
                entry.tries = 0;
            }
            if let Some(t) = self.last_consult.get(&d.dev_id) {
                if now.duration_since(*t) < CONSULT_RETRY {
                    continue;
                }
            }
            self.last_consult.insert(d.dev_id.clone(), now);
            let entry = self.negotiations.get_mut(&d.dev_id).unwrap();
            entry.tries += 1;
            batch.push((d.dev_id.clone(), mate_id, Negotiation {
                token: entry.token.clone(),
                rand_a: entry.rand_a.clone(),
                tries: entry.tries,
            }));
            if batch.len() >= CONSULT_BATCH {
                break;
            }
        }
        if batch.is_empty() {
            return;
        }
        let n = batch.len();
        let nonce = rand_bytes(12);
        let add = frame::build_add(PutType::ConsultRequest, n, self.seq, &self.main_mate_id, (36 + 24) * n as u32);
        let mut entries = Vec::with_capacity(n);
        for (_, mate_id, neg) in &batch {
            let mut plain = Vec::with_capacity(36);
            plain.extend_from_slice(&[0, 0, 0, 0]);
            plain.extend_from_slice(&neg.token);
            plain.extend_from_slice(&neg.rand_a);
            let (ct, tag) = crypto::gcm_encrypt(&self.consult_key, &nonce, &add, &plain);
            entries.push(frame::ConsultEntry { mate_id: *mate_id, cipher: ct, tag });
        }
        let fr = frame::build_consult_request(self.seq, &nonce, &self.main_mate_id, &entries);
        self.seq = self.seq.wrapping_add(1);
        self.broadcast(&fr);
    }

    /// Costruisce il frame business: UN sola add per il batch (come l'originale),
    /// I colori invariati richiedono soltanto un keepalive, non una coda di copie.
    pub fn build_pending_frame(&mut self) -> Option<Vec<u8>> {
        {
            let mut st = self.state.write();
            let stale: Vec<String> = st.frame_received_at.iter()
                .filter(|(_, time)| time.elapsed() > Duration::from_secs(2))
                .map(|(id, _)| id.clone()).collect();
            for id in stale {
                st.request_power(&id, false);
                st.latest.insert(id.clone(), [[0; 3]; 4]);
                st.release_after_send.insert(id.clone());
                st.frame_received_at.remove(&id);
            }
        }
        let pending: Vec<(DeviceCfg, [[u8; 3]; 4])> = {
            let st = self.state.read();
            st.config
                .devices
                .iter()
                .filter_map(|d| st.latest.get(&d.dev_id).map(|f| (d.clone(), *f)))
                .collect()
        };
        if pending.is_empty() {
            return None;
        }
        let mut values: Vec<(DeviceCfg, Vec<u8>, bool)> = Vec::new();
        for (d, leds) in &pending {
            let mate_id = self.sessions.mate_of(&d.dev_id, &self.main_mate_id);
            if !self.sessions.has(&mate_id) {
                continue;
            }
            let mut color_cfg = d.color.clone();
            color_cfg.rgbic = d.rgbic;
            let value = color::frame_to_values(leds, &color_cfg);
            let release = *leds == [[0; 3]; 4] && self.state.read().release_after_send.contains(&d.dev_id);
            if !release {
                if let Some(sent_at) = self.last_sent_at.get(&d.dev_id) {
                    let elapsed = sent_at.elapsed();
                    if self.last_sent.get(&d.dev_id) == Some(&value) && elapsed < Duration::from_secs(1) {
                        continue;
                    }
                    // Le Ball DP24 non devono ricevere la frequenza delle strisce RGBIC.
                    // Il nero interrompe subito lo stream, senza attendere il limite.
                    if !d.rgbic && *leds != [[0; 3]; 4] && elapsed < Duration::from_millis(100) {
                        continue;
                    }
                }
            }
            values.push((d.clone(), value, release));
        }
        if values.is_empty() {
            return None;
        }
        let total: u32 = values.iter().map(|(_, v, _)| v.len() as u32 + 24).sum();
        let nonce = rand_bytes(12);
        // UNA sola add per tutto il batch (bug fix review C1)
        let add = frame::build_add(PutType::BusinessRequest, values.len(), self.seq, &self.main_mate_id, total);
        let mut items = Vec::with_capacity(values.len());
        for (d, value, _release) in &values {
            let mate_id = self.sessions.mate_of(&d.dev_id, &self.main_mate_id);
            let key = match self.sessions.get(&mate_id) {
                Some(k) => k.clone(),
                None => continue,
            };
            let (ct, tag) = crypto::gcm_encrypt(&key, &nonce, &add, value);
            items.push(frame::BusinessItem { mate_id, cipher: ct, tag });

        }
        if items.is_empty() {
            return None;
        }
        let fr = frame::build_business_request(self.seq, &nonce, &self.main_mate_id, &items);
        self.seq = self.seq.wrapping_add(1);
        Some(fr)
    }

    pub fn send_pending(&mut self) {
        if let Some(fr) = self.build_pending_frame() {
            // Il broadcast Wi-Fi puo attendere il DTIM; usa gli IP dei beacon validi.
            // Mantieni il pacchetto cifrato identico: cambia soltanto la destinazione.
            let destinations = self.frame_destinations(&fr);
            let route_count = destinations.as_ref().map_or(0, Vec::len);
            if self.last_route_count.map(|count| count == 0) != Some(route_count == 0) {
                eprintln!("lscbridge: stream LAN: {} destinazioni dirette (0 = broadcast originale)", route_count);
                self.last_route_count = Some(route_count);
            }
            if let Some(destinations) = destinations {
                for ip in destinations {
                    for socket in &self.out40001 {
                        let _ = socket.send((Instant::now(), fr.clone(), Some(ip)));
                    }
                }
            } else {
                self.broadcast(&fr);
            }
        }
    }

    // Il worker conferma solo send_to riuscito, non la consegna fisica alla lampada.
    fn handle_sent(&mut self, packet: &[u8], target: Option<IpAddr>, sent_at: Instant) {
        if packet.len() < 42 || packet[10..14] != PutType::BusinessRequest.code() { return; }
        let devices = self.state.read().config.devices.clone();
        let mut offset = 38;
        while offset + 24 <= packet.len().saturating_sub(4) {
            let mate_id: [u8; 4] = match packet[offset..offset + 4].try_into() { Ok(id) => id, Err(_) => return };
            let len = u32::from_be_bytes(packet[offset + 4..offset + 8].try_into().unwrap()) as usize;
            let Some(end) = offset.checked_add(24).and_then(|n| n.checked_add(len)) else { return; };
            if end > packet.len() - 4 { return; }
            if let Some(device) = devices.iter().find(|d| self.sessions.mate_of(&d.dev_id, &self.main_mate_id) == mate_id) {
                // Un batch puo essere spedito a piu IP: conferma soltanto il destinatario.
                if target.is_none() || self.peers.get(&device.dev_id) == target.as_ref() {
                    if let Some(key) = self.sessions.get(&mate_id) {
                        if let Some(value) = crypto::gcm_decrypt(key, &packet[26..38], &packet[4..26],
                            &packet[offset + 8..offset + 8 + len], &packet[offset + 8 + len..end]) {
                            if self.last_sent_at.get(&device.dev_id).is_none_or(|previous| *previous <= sent_at) {
                                self.last_sent.insert(device.dev_id.clone(), value.clone());
                                self.last_sent_at.insert(device.dev_id.clone(), sent_at);
                                let mut st = self.state.write();
                                let mut config = device.color.clone();
                                config.rgbic = device.rgbic;
                                if st.release_after_send.contains(&device.dev_id)
                                    && st.latest.get(&device.dev_id) == Some(&[[0; 3]; 4])
                                    && value == color::frame_to_values(&[[0; 3]; 4], &config) {
                                    if st.power_confirmed.get(&device.dev_id) == Some(&false) {
                                        st.latest.remove(&device.dev_id);
                                    }
                                    st.release_after_send.remove(&device.dev_id);
                                    st.frame_received_at.remove(&device.dev_id);
                                }
                            }
                        }
                    }
                }
            }
            offset = end;
        }
    }

    fn frame_destinations(&self, frame: &[u8]) -> Option<Vec<IpAddr>> {
        let devices = self.state.read().config.devices.clone();
        let mut targets = Vec::new();
        let mut offset = 38;
        while offset + 24 <= frame.len().saturating_sub(4) {
            let mate_id: [u8; 4] = frame[offset..offset + 4].try_into().ok()?;
            let len = u32::from_be_bytes(frame[offset + 4..offset + 8].try_into().ok()?) as usize;
            let device = devices.iter().find(|d| self.sessions.mate_of(&d.dev_id, &self.main_mate_id) == mate_id)?;
            let ip = *self.peers.get(&device.dev_id)?;
            if !targets.contains(&ip) { targets.push(ip); }
            offset += 24 + len;
        }
        if targets.is_empty() || offset != frame.len() - 4 { None } else { Some(targets) }
    }

    /// Loop principale: smista i messaggi dai socket e scandisce il tick.
    pub fn run_mut(&mut self) {
        let mut last_tick = Instant::now();
        loop {
            while let Ok(msg) = self.rx.try_recv() {
                match msg {
                    RtMsg::Reply(f) => self.handle_reply(&f),
                    RtMsg::Sent(packet, target, sent_at) => self.handle_sent(&packet, target, sent_at),
                    RtMsg::Online(gw, ip) => {
                        if self.state.read().config.device(&gw).is_some() { self.peers.insert(gw.clone(), ip); }
                        self.handle_online(gw);
                    },
                }
            }
            if last_tick.elapsed() >= self.frame_period {
                self.consult_missing();
                self.send_pending();
                last_tick = Instant::now();
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn handle_online(&mut self, dev_id: String) {
        let mate_id = self.sessions.mate_of(&dev_id, &self.main_mate_id);
        if !self.state.read().config.devices.iter().any(|d| d.dev_id == dev_id) {
            return;
        }
        self.last_online.insert(dev_id.clone(), Instant::now());
        // Un beacon prova la presenza, non l'autenticazione necessaria allo stream.
        if !self.sessions.has(&mate_id) {
            return;
        }
        let mut st = self.state.write();
        if !st.online.contains(&dev_id) {
            st.online.push(dev_id);
        }
    }
}

/// Thread per socket 40001: inoltro dei frame da inviare + ricezione reply.
fn socket_thread(socket: UdpSocket, tx: Sender<RtMsg>, rx_out: Receiver<(Instant, Vec<u8>, Option<IpAddr>)>, is_online: bool, peer: String) {
    let _ = is_online;
    let mut buf = [0u8; 2048];
    let mut count = 0u32;
    let mut max_queue_ms = 0u128;
    loop {
        while let Ok((queued, data, target)) = rx_out.try_recv() {
            // Dopo un arresto del thread scarta i colori vecchi, mai le negoziazioni.
            if stale_color(&data, queued, Instant::now()) { continue; }
            max_queue_ms = max_queue_ms.max(queued.elapsed().as_millis());
            count = count.wrapping_add(1);
            if count % 300 == 0 {
                eprintln!("lscbridge: UDP intervallo diagnostico: {} pacchetti, coda massima {} ms", count, max_queue_ms);
                max_queue_ms = 0;
            }
            let destination = target.map(|ip| ip.to_string()).unwrap_or_else(|| peer.clone());
            match socket.send_to(&data, (destination.as_str(), BROADCAST_PORT)) {
                Ok(bytes) if bytes == data.len() => {
                    if data.len() >= 14 && data[10..14] == PutType::BusinessRequest.code() {
                        let _ = tx.send(RtMsg::Sent(data, target, Instant::now()));
                    }
                }
                Ok(_) => eprintln!("lscbridge: invio UDP incompleto"),
                Err(e) => eprintln!("lscbridge: invio UDP verso {}:{} fallito: {}", peer, BROADCAST_PORT, e),
            }
        }
        match socket.recv_from(&mut buf) {
            Ok((n, _addr)) => {
                let data = buf[..n].to_vec();
                if is_device_frame(&data) {
                    let _ = tx.send(RtMsg::Reply(data));
                }
            }
            Err(ref e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// Thread solo-ricezione per 6667 (beacon online).
fn recv_only_thread(socket: UdpSocket, tx: Sender<RtMsg>) {
    let mut buf = [0u8; 2048];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((n, _addr)) => {
                let data = buf[..n].to_vec();
                if data.len() >= 14 && data[0..4] == HEADER && data[10..14] == [0, 0, 0, 0x13] {
                    if let Some(gw) = frame::parse_online_report(&data, &ONLINE_KEY) {
                        let _ = tx.send(RtMsg::Online(gw, _addr.ip()));
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn is_device_frame(data: &[u8]) -> bool {
    data.len() >= 46 && data[0..4] == HEADER && data[data.len() - 4..] == TAIL
}

fn stale_color(data: &[u8], queued: Instant, now: Instant) -> bool {
    data.len() >= 14 && data[..4] == HEADER && data[10..14] == PutType::BusinessRequest.code()
        && now.duration_since(queued) > Duration::from_millis(100)
}

pub fn rand_bytes(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut b);
    b
}

/// MAC del primo adattatore fisico (formato getmac: "AC-1F-6B-…" o "AC:1F:…").
/// Il seed usato dall'app è lo stesso MAC senza separatori.
fn first_mac_hex() -> String {
    if let Ok(out) = std::process::Command::new("getmac.exe")
        .args(["/fo", "csv", "/nh"])
        .output()
    {
        if let Ok(s) = String::from_utf8(out.stdout) {
            for line in s.lines() {
                if let Some(tok) = line
                    .split(['"', ','])
                    .map(|t| t.replace('-', "").replace(':', ""))
                    .find(|t| t.len() == 12 && t.chars().all(|c| c.is_ascii_hexdigit()))
                {
                    return tok.to_lowercase();
                }
            }
        }
    }
    "020000000000".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceCfg};
    use crate::proto::frame::{self, PutType};

    fn test_runtime_with(local_key: &str) -> UdpRuntime {
        let mut config = Config::default();
        let mut d = DeviceCfg::new("bf7f0b6d52a1c3e4f5", "Lampada test", true);
        d.local_key = Some(local_key.to_string());
        config.devices.push(d);
        UdpRuntime::new(SharedState::new(config), 30)
    }

    /// Simula la risposta di un dispositivo al consult.
    fn simulate_device_reply(consult_frame: &[u8], local_key_bytes: &[u8]) -> Vec<u8> {
        let add = &consult_frame[4..26];
        let nonce = &consult_frame[26..38];
        // primo blocco device: mate(4) len(4) ct tag
        let mate: [u8; 4] = consult_frame[38..42].try_into().unwrap();
        let len = u32::from_be_bytes(consult_frame[42..46].try_into().unwrap()) as usize;
        let ct = &consult_frame[46..46 + len];
        let tag = &consult_frame[46 + len..46 + len + 16];

        let key = util::hex_decode(CONSULT_KEY_HEX).unwrap();
        let plain = crypto::gcm_decrypt(&key, nonce, add, ct, tag).expect("device decrypt consult");
        let token = &plain[4..20];
        let rand_a = &plain[20..36];

        let negotiate_key = crypto::negotiate_key(local_key_bytes, token);
        let rand_b = rand_bytes(16);
        let session = crypto::session_key(&negotiate_key, rand_a, &util::xor(rand_a, &rand_b));
        let mut payload = Vec::new();
        payload.extend_from_slice(&rand_b);
        payload.extend_from_slice(&crypto::hmac_sha256(&negotiate_key, rand_a));
        payload.extend_from_slice(&crypto::hmac_sha256(&negotiate_key, &session));
        let mut add_dev = add.to_vec();
        add_dev[10..14].copy_from_slice(&mate); // il device rimette il proprio mate nell'AAD
        let (ect, etag) = crypto::gcm_encrypt(&negotiate_key, nonce, &add_dev, &payload);
        let mut fr = Vec::new();
        fr.extend_from_slice(&HEADER);
        fr.extend_from_slice(&add_dev);
        fr.extend_from_slice(nonce);
        fr.extend_from_slice(&[0, 0, 0, 0]); // id ignorato
        let lenhex = util::pad_hex(4, &format!("{:x}", ect.len()), false);
        fr.extend_from_slice(&util::hex_decode(&lenhex).unwrap());
        fr.extend_from_slice(&ect);
        fr.extend_from_slice(&etag);
        fr.extend_from_slice(&TAIL);
        fr
    }

    #[test]
    fn re_pairing_invalidates_only_the_changed_device_session() {
        let mut rt = test_runtime_with("abcdefghijklmnop");
        let id = "bf7f0b6d52a1c3e4f5";
        let other = "other-device";
        rt.state.write().config.devices.push(DeviceCfg::new(other, "Other", true));
        let changed_mate = mate::child_mate_id(id, &rt.main_mate_id);
        let other_mate = mate::child_mate_id(other, &rt.main_mate_id);
        rt.sessions.insert(changed_mate, vec![1; 16]);
        rt.sessions.insert(other_mate, vec![2; 16]);
        {
            let mut state = rt.state.write();
            state.online = vec![id.into(), other.into()];
            state.renegotiate.insert(id.into());
            state.request_power(id, true);
        }
        rt.consult_missing();
        assert!(!rt.sessions.has(&changed_mate));
        assert!(rt.sessions.has(&other_mate));
        assert!(rt.negotiations.contains_key(id));
        assert!(!rt.state.read().online.contains(&id.to_string()));
        assert!(!rt.state.read().power_requested.contains_key(id));
    }

    #[test]
    fn beacon_does_not_report_an_unauthenticated_light_as_ready() {
        let mut rt = test_runtime_with("abcdefghijklmnop");
        let id = "bf7f0b6d52a1c3e4f5";
        rt.handle_online(id.into());
        assert!(rt.last_online.contains_key(id));
        assert!(rt.state.read().online.is_empty());
        let m = mate::child_mate_id(id, &rt.main_mate_id);
        rt.sessions.insert(m, vec![7; 16]);
        rt.handle_online(id.into());
        rt.handle_online(id.into());
        assert_eq!(rt.state.read().online, vec![id.to_string()]);
        rt.handle_online("unknown".into());
        assert!(!rt.last_online.contains_key("unknown"));
    }

    #[test]
    fn local_key_ascii_matches_vendor_derivation() {
        // vendor: getBuffer(getByteDataFromLen(16, stringtoHex(localKey), true))
        assert_eq!(local_key_bytes("abcdefghijklmnop"), b"abcdefghijklmnop".to_vec());
        assert_eq!(local_key_bytes("short").len(), 16);
        assert_eq!(&local_key_bytes("short")[0..5], b"short");
    }

    #[test]
    fn loopback_negotiation_creates_session() {
        let local_key = "abcdefghijklmnop";
        let mut rt = test_runtime_with(local_key);
        let mate_id = mate::child_mate_id("bf7f0b6d52a1c3e4f5", &rt.main_mate_id);

        let token = rand_bytes(16);
        let rand_a = rand_bytes(16);
        rt.negotiations.insert(
            "bf7f0b6d52a1c3e4f5".into(),
            Negotiation { token: token.clone(), rand_a: rand_a.clone(), tries: 1 },
        );
        let nonce = rand_bytes(12);
        let mut plain = vec![0, 0, 0, 0];
        plain.extend_from_slice(&token);
        plain.extend_from_slice(&rand_a);
        let add = frame::build_add(PutType::ConsultRequest, 1, 1, &rt.main_mate_id, 36 + 24);
        let (ct, tag) = crypto::gcm_encrypt(&rt.consult_key, &nonce, &add, &plain);
        let consult = frame::build_consult_request(
            1,
            &nonce,
            &rt.main_mate_id,
            &[frame::ConsultEntry { mate_id, cipher: ct, tag }],
        );
        let reply = simulate_device_reply(&consult, &local_key_bytes(local_key));
        rt.handle_reply(&reply);
        assert!(rt.sessions.has(&mate_id), "sessione non stabilita dal loopback");
        assert!(rt.state.read().online.contains(&"bf7f0b6d52a1c3e4f5".to_string()));
    }

    #[test]
    fn business_frame_two_devices_share_one_aad() {
        // review C1: l'add deve essere unica per il batch
        let mut state = SharedState::new(Config::default());
        for id in ["devAAAAAAAAAAAAAAA1", "devBBBBBBBBBBBBBBB2"] {
            let mut d = DeviceCfg::new(id, id, true);
            d.local_key = Some("abcdefghijklmnop".into());
            state.write().config.devices.push(d);
        }
        let mut rt = UdpRuntime::new(state, 30);
        for id in ["devAAAAAAAAAAAAAAA1", "devBBBBBBBBBBBBBBB2"] {
            let m = mate::child_mate_id(id, &rt.main_mate_id);
            rt.sessions.register_dev(id, m);
            rt.sessions.insert(m, rand_bytes(16));
            rt.state
                .write()
                .latest
                .insert(id.into(), [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]]);
        }
        let fr = rt.build_pending_frame().expect("frame batch");
        let add = &fr[4..26];
        let nonce = &fr[26..38];
        let mut off = 38usize;
        let mut blocks = 0;
        while off + 4 + 16 <= fr.len() - 4 {
            let len = u32::from_be_bytes(fr[off + 4..off + 8].try_into().unwrap()) as usize;
            let ct = &fr[off + 8..off + 8 + len];
            let tag = &fr[off + 8 + len..off + 8 + len + 16];
            assert_eq!(add.len(), 22);
            assert_eq!(nonce.len(), 12);
            assert!(ct.len() > 0);
            blocks += 1;
            off += 8 + len + 16;
        }
        assert_eq!(blocks, 2, "devono esserci 2 blocchi device nello stesso frame");
        // verifica che entrambi i blocchi decifrino con la STESSA add (bug C1)
        let mut off = 38usize;
        for id in ["devAAAAAAAAAAAAAAA1", "devBBBBBBBBBBBBBBB2"] {
            let m = mate::child_mate_id(id, &rt.main_mate_id);
            let key = rt.sessions.get(&m).unwrap().clone();
            let len = u32::from_be_bytes(fr[off + 4..off + 8].try_into().unwrap()) as usize;
            let ct = &fr[off + 8..off + 8 + len];
            let tag = &fr[off + 8 + len..off + 8 + len + 16];
            let pt = crypto::gcm_decrypt(&key, nonce, add, ct, tag).expect("decrypt blocco");
            assert!(!pt.is_empty(), "valore dp decifrato per {}", id);
            off += 8 + len + 16;
        }
    }

    #[test]
    fn identical_frame_is_coalesced_until_keepalive() {
        let mut rt = test_runtime_with("abcdefghijklmnop");
        let mate_id = mate::child_mate_id("bf7f0b6d52a1c3e4f5", &rt.main_mate_id);
        rt.sessions.register_dev("bf7f0b6d52a1c3e4f5", mate_id);
        rt.sessions.insert(mate_id, rand_bytes(16));
        rt.state.write().latest.insert(
            "bf7f0b6d52a1c3e4f5".into(),
            [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]],
        );
        let first = rt.build_pending_frame().expect("primo frame costruito");
        assert!(rt.build_pending_frame().is_some(), "accodare non significa inviare");
        rt.handle_sent(&first, None, Instant::now());
        assert!(rt.build_pending_frame().is_none(), "nessun duplicato del colore fisso");
        rt.last_sent_at.insert("bf7f0b6d52a1c3e4f5".into(), Instant::now() - Duration::from_secs(1));
        assert!(rt.build_pending_frame().is_some(), "keepalive dopo un secondo");
    }

    #[test]
    fn unsent_or_wrong_destination_never_suppresses_current_color() {
        let mut rt = test_runtime_with("abcdefghijklmnop");
        let id = "bf7f0b6d52a1c3e4f5";
        let mate_id = mate::child_mate_id(id, &rt.main_mate_id);
        rt.sessions.insert(mate_id, vec![7; 16]);
        let expected: IpAddr = "192.0.2.10".parse().unwrap();
        rt.peers.insert(id.into(), expected);
        rt.state.write().latest.insert(id.into(), [[255, 0, 0]; 4]);
        let unsent = rt.build_pending_frame().unwrap();
        assert!(stale_color(&unsent, Instant::now() - Duration::from_secs(1), Instant::now()));
        assert!(!rt.last_sent.contains_key(id));
        rt.state.write().latest.insert(id.into(), [[0, 255, 0]; 4]);
        let current = rt.build_pending_frame().unwrap();
        rt.handle_sent(&current, Some("192.0.2.11".parse().unwrap()), Instant::now());
        assert!(rt.build_pending_frame().is_some(), "invio ad altro destinatario non conferma la lampada");
        rt.handle_sent(&current, Some(expected), Instant::now());
        assert!(rt.build_pending_frame().is_none(), "solo invio riuscito al destinatario avvia deduplica");
    }

    #[test]
    fn simple_light_skips_old_colors_and_black_bypasses_rate_limit() {
        let mut rt = test_runtime_with("abcdefghijklmnop");
        let id = "bf7f0b6d52a1c3e4f5";
        rt.state.write().config.devices[0].rgbic = false;
        let mate_id = mate::child_mate_id(id, &rt.main_mate_id);
        rt.sessions.insert(mate_id, vec![7; 16]);
        rt.state.write().latest.insert(id.into(), [[255, 0, 0]; 4]);
        let first = rt.build_pending_frame().unwrap();
        rt.handle_sent(&first, None, Instant::now());
        rt.state.write().latest.insert(id.into(), [[0, 255, 0]; 4]);
        assert!(rt.build_pending_frame().is_none());
        rt.state.write().latest.insert(id.into(), [[0, 0, 255]; 4]);
        assert!(rt.build_pending_frame().is_none());
        rt.last_sent_at.insert(id.into(), Instant::now() - Duration::from_millis(100));
        let packet = rt.build_pending_frame().unwrap();
        let len = u32::from_be_bytes(packet[42..46].try_into().unwrap()) as usize;
        let value = crypto::gcm_decrypt(&[7; 16], &packet[26..38], &packet[4..26],
            &packet[46..46+len], &packet[46+len..46+len+16]).unwrap();
        assert_eq!(value, color::dp::dp24_value((240, 1000, 1000)), "solo il blu attuale, mai il verde saltato");
        rt.state.write().latest.insert(id.into(), [[0; 3]; 4]);
        assert!(rt.build_pending_frame().is_some(), "nero immediato");
    }

    #[test]
    fn business_frame_decrypts_to_dp24_of_first_zone() {
        let mut rt = test_runtime_with("abcdefghijklmnop");
        rt.state.write().config.devices[0].rgbic = false;
        let mate_id = mate::child_mate_id("bf7f0b6d52a1c3e4f5", &rt.main_mate_id);
        rt.sessions.register_dev("bf7f0b6d52a1c3e4f5", mate_id);
        let session = rand_bytes(16);
        rt.sessions.insert(mate_id, session.clone());
        rt.state.write().latest.insert(
            "bf7f0b6d52a1c3e4f5".into(),
            [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]],
        );

        let fr = rt.build_pending_frame().expect("frame mancante");
        let add = &fr[4..26];
        let nonce = &fr[26..38];
        let len = u32::from_be_bytes(fr[42..46].try_into().unwrap()) as usize;
        let ct = &fr[46..46 + len];
        let tag = &fr[46 + len..46 + len + 16];
        let value = crypto::gcm_decrypt(&session, nonce, add, ct, tag).expect("device decrypt business");
        assert_eq!(hex::encode(&value), "0000646400000100");
    }

    #[test]
    fn session_store_retain_drops_unknown() {
        let main = [1, 2, 3, 4];
        let mut s = SessionStore::default();
        let m = mate::child_mate_id("dev1", &main);
        s.register_dev("dev1", m);
        s.insert(m, vec![9; 16]);
        s.retain_valid_devices(&["dev1".into()], &main);
        assert!(s.has(&m));
        s.retain_valid_devices(&[], &main);
        assert!(!s.has(&m));
    }

    #[test]
    fn configured_rgbic_uses_four_zones() {
        let mut rt = test_runtime_with("abcdefghijklmnop");
        let id = "bf7f0b6d52a1c3e4f5";
        let mate_id = mate::child_mate_id(id, &rt.main_mate_id);
        let key = vec![7; 16];
        rt.sessions.insert(mate_id, key.clone());
        let leds = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];
        rt.state.write().latest.insert(id.into(), leds);
        let fr = rt.build_pending_frame().unwrap();
        let len = u32::from_be_bytes(fr[42..46].try_into().unwrap()) as usize;
        let value = crypto::gcm_decrypt(&key, &fr[26..38], &fr[4..26], &fr[46..46+len], &fr[46+len..46+len+16]).unwrap();
        let cfg = color::DeviceColorCfg { rgbic: true, ..Default::default() };
        assert_eq!(value, color::frame_to_values(&leds, &cfg));
    }

    #[test]
    fn colors_start_on_lan_without_waiting_for_cloud() {
        let mut rt = test_runtime_with("abcdefghijklmnop");
        let id = "bf7f0b6d52a1c3e4f5";
        let mate_id = mate::child_mate_id(id, &rt.main_mate_id);
        rt.sessions.insert(mate_id, vec![7; 16]);
        {
            let mut state = rt.state.write();
            state.config.session = Some(crate::config::SessionCfg {sid:"test".into(), ecode:"test".into()});
            state.request_power(id, true);
            state.latest.insert(id.into(), [[255; 3]; 4]);
        }
        assert!(rt.build_pending_frame().is_some());
        assert!(rt.state.read().pending_power.is_empty(), "nessun ON cloud durante gli effetti");
        {
            let mut state = rt.state.write();
            state.request_power(id, false);
            state.latest.insert(id.into(), [[0; 3]; 4]);
        }
        assert!(rt.build_pending_frame().is_some(), "nero disponibile anche prima della conferma off");
    }

    #[test]
    fn release_encodes_black_before_removing_frame() {
        let mut rt = test_runtime_with("abcdefghijklmnop");
        let id = "bf7f0b6d52a1c3e4f5";
        let mate_id = mate::child_mate_id(id, &rt.main_mate_id);
        let key = vec![7; 16];
        rt.sessions.insert(mate_id, key.clone());
        rt.state.write().latest.insert(id.into(), [[0; 3]; 4]);
        rt.state.write().release_after_send.insert(id.into());
        let fr = rt.build_pending_frame().expect("nero non trasmesso");
        let len = u32::from_be_bytes(fr[42..46].try_into().unwrap()) as usize;
        let value = crypto::gcm_decrypt(&key, &fr[26..38], &fr[4..26], &fr[46..46+len], &fr[46+len..46+len+16]).unwrap();
        let cfg = color::DeviceColorCfg { rgbic: true, ..Default::default() };
        assert_eq!(value, color::frame_to_values(&[[0; 3]; 4], &cfg));
        assert!(rt.state.read().latest.contains_key(id), "nero resta fino a send_to riuscito");
        rt.handle_sent(&fr, None, Instant::now());
        assert!(rt.state.read().latest.contains_key(id), "nero mantenuto prima di conferma DP20");
        rt.state.write().power_confirmed.insert(id.into(), false);
        rt.state.write().release_after_send.insert(id.into());
        rt.handle_sent(&fr, None, Instant::now());
        assert!(!rt.state.read().latest.contains_key(id));
        assert!(rt.build_pending_frame().is_none());
    }

    #[test]
    fn lost_renderer_queues_black_after_two_seconds() {
        let mut rt = test_runtime_with("abcdefghijklmnop");
        let id = "bf7f0b6d52a1c3e4f5";
        let mate_id = mate::child_mate_id(id, &rt.main_mate_id);
        rt.sessions.insert(mate_id, vec![7;16]);
        rt.state.write().latest.insert(id.into(), [[0,0,255];4]);
        rt.state.write().frame_received_at.insert(id.into(), Instant::now() - Duration::from_secs(3));
        let black = rt.build_pending_frame().unwrap();
        assert!(rt.state.read().latest.contains_key(id));
        rt.handle_sent(&black, None, Instant::now());
        assert!(rt.state.read().latest.contains_key(id), "nero mantenuto durante retry cloud");
        rt.state.write().power_confirmed.insert(id.into(), false);
        rt.state.write().release_after_send.insert(id.into());
        rt.handle_sent(&black, None, Instant::now());
        assert!(!rt.state.read().latest.contains_key(id));
        assert!(rt.build_pending_frame().is_none());
    }

    #[test]
    fn missing_beacon_preserves_session_and_recovers_without_renegotiation() {
        let mut rt = test_runtime_with("abcdefghijklmnop");
        let id = "bf7f0b6d52a1c3e4f5";
        let mate_id = mate::child_mate_id(id, &rt.main_mate_id);
        rt.sessions.insert(mate_id, vec![7; 16]);
        rt.last_sent.insert(id.into(), vec![1]);
        rt.last_online.insert(id.into(), Instant::now() - ONLINE_TIMEOUT - Duration::from_secs(1));
        rt.consult_missing();
        assert!(rt.sessions.has(&mate_id));
        assert!(!rt.last_sent.contains_key(id));
        assert!(!rt.negotiations.contains_key(id));
        assert!(!rt.state.read().online.contains(&id.to_string()));
        rt.handle_online(id.into());
        assert!(rt.state.read().online.contains(&id.to_string()));
    }

    #[test]
    fn stalled_socket_drops_old_colors_but_preserves_control_packets() {
        let now = Instant::now();
        let old = now - Duration::from_millis(101);
        let mut packet = frame::build_consult_request(1, &[0; 12], &[1; 4], &[]);
        assert!(!stale_color(&packet, old, now));
        packet[10..14].copy_from_slice(&PutType::BusinessRequest.code());
        assert!(stale_color(&packet, old, now));
        assert!(!stale_color(&packet, now - Duration::from_millis(99), now));
    }

    #[test]
    fn business_targets_known_beacon_ip_without_changing_ciphertext() {
        let mut rt = test_runtime_with("abcdefghijklmnop");
        let id = "bf7f0b6d52a1c3e4f5";
        let mate_id = mate::child_mate_id(id, &rt.main_mate_id);
        rt.sessions.insert(mate_id, vec![7; 16]);
        rt.state.write().latest.insert(id.into(), [[255; 3]; 4]);
        let frame = rt.build_pending_frame().unwrap();
        assert!(rt.frame_destinations(&frame).is_none(), "IP ignoto mantiene fallback originale");
        let target: IpAddr = "192.0.2.10".parse().unwrap();
        rt.peers.insert(id.into(), target);
        assert_eq!(rt.frame_destinations(&frame), Some(vec![target]));
        let (sender, receiver) = channel();
        rt.out40001 = vec![sender];
        rt.last_sent_at.clear();
        rt.send_pending();
        let (_, sent, peer) = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(peer, Some(target));
        let len = u32::from_be_bytes(sent[42..46].try_into().unwrap()) as usize;
        let plaintext = crypto::gcm_decrypt(&[7; 16], &sent[26..38], &sent[4..26],
            &sent[46..46+len], &sent[46+len..46+len+16]).unwrap();
        assert_eq!(plaintext, color::frame_to_values(&[[255; 3]; 4], &color::DeviceColorCfg {rgbic: true, ..Default::default()}));
    }

    #[test]
    fn udp_worker_delivers_datagram_to_protocol_port() {
        let receiver = UdpSocket::bind(("127.0.0.1", BROADCAST_PORT)).unwrap();
        receiver.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender.set_read_timeout(Some(Duration::from_millis(2))).unwrap();
        let (tx, _rx) = channel();
        let (out, input) = channel();
        thread::spawn(move || socket_thread(sender, tx, input, false, "127.0.0.1".into()));
        out.send((Instant::now(), vec![1, 2, 3], None)).unwrap();
        let mut buf = [0; 16];
        let (n, _) = receiver.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..n], &[1, 2, 3]);
        // Misura anche il percorso a socket inattivo, dove scatta il timeout Windows.
        for seq in 0..12 {
            thread::sleep(Duration::from_millis(40));
            let sent = Instant::now();
            out.send((Instant::now(), vec![seq], None)).unwrap();
            let (n, _) = receiver.recv_from(&mut buf).unwrap();
            assert_eq!(&buf[..n], &[seq]);
            assert!(sent.elapsed() < Duration::from_millis(150), "worker UDP in ritardo: {:?}", sent.elapsed());
        }
    }
}
