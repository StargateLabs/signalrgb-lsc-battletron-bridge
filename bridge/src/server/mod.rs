// Control server TCP su 127.0.0.1 — protocollo con il plugin SignalRGB.
// Framing: [u32 len][json]. Comandi: HELLO, LIST, FRAME, RELEASE.
use crate::state::SharedState;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub const PROTO_VERSION: u32 = 1;

#[derive(Deserialize)]
struct Request {
    cmd: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    leds: Option<Vec<[u8; 3]>>,
}

#[derive(Serialize)]
struct HelloReply {
    ok: bool,
    app: &'static str,
    ver: &'static str,
    proto: u32,
}
#[derive(Serialize)]
struct ListReply {
    ok: bool,
    devices: Vec<crate::state::DeviceView>,
}
#[derive(Serialize)]
struct OkReply {
    ok: bool,
}
#[derive(Serialize)]
struct ErrReply {
    error: String,
}

pub struct ControlServer {
    pub port: u16,
}

impl ControlServer {
    pub fn bind(state: SharedState, port: u16) -> std::io::Result<(Self, Arc<AtomicBool>)> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let actual = listener.local_addr()?.port();
        let udp = UdpSocket::bind(("127.0.0.1", actual))?;
        udp.set_read_timeout(Some(Duration::from_millis(250)))?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let udp_stop = stop.clone();
        let udp_state = state.clone();
        thread::spawn(move || serve_udp(udp, udp_state, udp_stop));
        let power_state = state.clone();
        let power_stop = stop.clone();
        thread::spawn(move || serve_power(power_state, power_stop));
        thread::spawn(move || serve(listener, state, stop2));
        Ok((ControlServer { port: actual }, stop))
    }
}

fn serve_udp(socket: UdpSocket, state: SharedState, stop: Arc<AtomicBool>) {
    let mut buf = [0u8; 65536];
    while !stop.load(Ordering::SeqCst) {
        let (len, peer) = match socket.recv_from(&mut buf) {
            Ok(packet) => packet,
            Err(_) => continue,
        };
        if len < 4 || u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize != len - 4 {
            continue;
        }
        let req: Request = match serde_json::from_slice(&buf[4..len]) {
            Ok(req) => req,
            Err(_) => continue,
        };
        // Colori e rilascio non richiedono ACK: nessuna attesa nel render loop.
        let reply_needed = !matches!(req.cmd.as_str(), "FRAME" | "HOLD" | "RELEASE");
        let reply = execute_request(req, &state);
        if reply_needed {
            let mut packet = (reply.len() as u32).to_be_bytes().to_vec();
            packet.extend_from_slice(reply.as_bytes());
            let _ = socket.send_to(&packet, peer);
        }
    }
}

fn serve_power(state: SharedState, stop: Arc<AtomicBool>) {
    let mut client: Option<crate::cloud::TuyaClient> = None;
    let mut gid: Option<String> = None;
    let mut retry: std::collections::HashMap<String, (u32, Instant)> = std::collections::HashMap::new();
    while !stop.load(Ordering::SeqCst) {
        let pending = {
            let mut st = state.write();
            st.pending_power.iter().find(|(id, _)| retry.get(*id).is_none_or(|(_, time)| Instant::now() >= *time))
                .map(|(id, on)| (id.clone(), *on)).map(|(id, on)| {
                st.pending_power.remove(&id);
                (id, on, st.config.clone())
            })
        };
        if let Some((id, on, config)) = pending {
            let result = (|| -> Result<(), String> {
                let session = config.session.as_ref().ok_or("sessione cloud assente: solo nero LAN disponibile")?;
                if client.as_ref().map(|c| (&c.region, c.sid.as_ref())) != Some((&config.region, Some(&session.sid))) {
                    let mut c = crate::cloud::TuyaClient::new(&config.region);
                    c.sid = Some(session.sid.clone());
                    c.ecode = Some(session.ecode.clone());
                    client = Some(c);
                    gid = None;
                }
                let c = client.as_mut().unwrap();
                if gid.is_none() { gid = Some(c.first_home_id()?); }
                // La ricerca della casa puo durare: non pubblicare OFF superato da un nuovo stream.
                if state.read().power_requested.get(&id) != Some(&on) { return Ok(()); }
                c.set_light_power(gid.as_ref().unwrap(), &id, on)
            })();
            match result {
                Ok(()) => {
                    retry.remove(&id);
                    if state.read().power_requested.get(&id) == Some(&on) {
                        eprintln!("lscbridge: DP20 {} confermato per {}", if on {"on"} else {"off"}, id);
                    }
                }
                Err(e) => {
                    let attempt = retry.get(&id).map_or(1, |(n, _)| n.saturating_add(1));
                    retry.insert(id.clone(), (attempt, Instant::now() + Duration::from_secs(1u64 << attempt.min(5))));
                    let mut st = state.write();
                    if config.session.is_some() && st.power_requested.get(&id) == Some(&on) {
                        st.pending_power.entry(id.clone()).or_insert(on);
                    }
                    eprintln!("lscbridge: alimentazione {}: {}; nuovo tentativo con backoff", id, e);
                }
            }
        } else {
            thread::sleep(Duration::from_millis(50));
        }
    }
}

fn serve(listener: TcpListener, state: SharedState, stop: Arc<AtomicBool>) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match stream {
            Ok(s) => {
                let st = state.clone();
                thread::spawn(move || handle(s, st));
            }
            Err(_) => continue,
        }
    }
}

fn read_frame(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut lenb = [0u8; 4];
    stream.read_exact(&mut lenb)?;
    let len = u32::from_be_bytes(lenb) as usize;
    if len > 64 * 1024 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame troppo grande"));
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

fn write_frame(stream: &mut TcpStream, body: &str) -> std::io::Result<()> {
    let bytes = body.as_bytes();
    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
    stream.write_all(bytes)?;
    stream.flush()
}

fn handle(mut stream: TcpStream, state: SharedState) {
    let peer = stream.peer_addr().ok();
    let mut first_request = true;
    let mut first_frame = true;
    loop {
        let frame = match read_frame(&mut stream) {
            Ok(f) => f,
            Err(_) => return, // connessione chiusa
        };
        let req: Request = match serde_json::from_slice(&frame) {
            Ok(r) => r,
            Err(e) => {
                let _ = write_frame(&mut stream, &serde_json::to_string(&ErrReply { error: format!("json: {}", e) }).unwrap());
                continue;
            }
        };
        let command = req.cmd.clone();
        if command == "FRAME" && first_frame {
            eprintln!("lscbridge: primo frame SignalRGB per {}", req.id.as_deref().unwrap_or("senza id"));
            first_frame = false;
        }
        let reply = execute_request(req, &state);
        if first_request {
            eprintln!("lscbridge: client TCP {:?}, comando {}", peer, command);
            first_request = false;
        }
        if write_frame(&mut stream, &reply).is_err() {
            return;
        }
        let _ = peer; // (debug futura)
    }
}

fn execute_request(req: Request, state: &SharedState) -> String {
        match req.cmd.as_str() {
            "HELLO" => serde_json::to_string(&HelloReply {
                ok: true,
                app: "lscbridge",
                ver: env!("CARGO_PKG_VERSION"),
                proto: PROTO_VERSION,
            })
            .unwrap(),
            "LIST" => serde_json::to_string(&ListReply {
                ok: true,
                devices: state.device_views(),
            })
            .unwrap(),
            "RELOAD" => match crate::config::Config::load() {
                Ok(config) => {
                    state.write().replace_config(config);
                    serde_json::to_string(&OkReply { ok: true }).unwrap()
                }
                Err(e) => serde_json::to_string(&ErrReply { error: format!("config: {}", e) }).unwrap(),
            },
            "RECONNECT" => {
                let mut st = state.write();
                let ids: Vec<String> = st.config.devices.iter()
                    .filter(|d| req.id.as_ref().is_none_or(|id| id == &d.dev_id))
                    .map(|d| d.dev_id.clone()).collect();
                if ids.is_empty() {
                    serde_json::to_string(&ErrReply {error: "device sconosciuto".into()}).unwrap()
                } else {
                    st.renegotiate.extend(ids);
                    serde_json::to_string(&OkReply {ok: true}).unwrap()
                }
            }
            "FRAME" | "HOLD" => {
                let id = match req.id {
                    Some(i) => i,
                    None => serde_json::to_string(&ErrReply { error: "id mancante".into() }).unwrap(),
                };
                match req.leds {
                    Some(leds) if leds.len() == 4 => {
                        let mut arr: [[u8; 3]; 4] = [[0; 3]; 4];
                        for (i, l) in leds.iter().enumerate() {
                            arr[i] = *l;
                        }
                        {
                            let st = state.read();
                            if !st.config.devices.iter().any(|d| d.dev_id == id) {
                                serde_json::to_string(&ErrReply { error: "device sconosciuto".into() }).unwrap()
                            } else {
                                drop(st);
                                let mut st = state.write();
                                // Brief effect blacks stay LAN-only; persistent black also disables DP20.
                                if arr == [[0; 3]; 4] {
                                    let since = *st.black_since.entry(id.clone()).or_insert_with(Instant::now);
                                    if since.elapsed() >= Duration::from_secs(5) {
                                        st.request_power(&id, false);
                                    }
                                } else {
                                    st.black_since.remove(&id);
                                    st.request_power(&id, true);
                                }
                                if req.cmd == "HOLD" && arr == [[0; 3]; 4] {
                                    st.request_power(&id, false);
                                }
                                st.release_after_send.remove(&id);
                                if req.cmd == "HOLD" {
                                    st.frame_received_at.remove(&id);
                                } else {
                                    st.frame_received_at.insert(id.clone(), Instant::now());
                                }
                                // Once DP20 OFF is requested, do not wake it again with LAN keepalives.
                                if arr == [[0; 3]; 4] && st.power_requested.get(&id) == Some(&false) {
                                    st.latest.remove(&id);
                                } else {
                                    st.latest.insert(id, arr);
                                }
                                serde_json::to_string(&OkReply { ok: true }).unwrap()
                            }
                        }
                    }
                    _ => serde_json::to_string(&ErrReply { error: "leds non valido".into() }).unwrap(),
                }
            }
            "RELEASE" => {
                if let Some(id) = req.id {
                    let mut st = state.write();
                    if st.config.device(&id).is_some() {
                        st.request_power(&id, false);
                        st.latest.insert(id.clone(), [[0; 3]; 4]);
                        st.release_after_send.insert(id);
                    }
                }
                serde_json::to_string(&OkReply { ok: true }).unwrap()
            }
            other => {
                eprintln!("lscbridge: comando sconosciuto: {}", other);
                serde_json::to_string(&ErrReply { error: format!("comando sconosciuto: {}", other) }).unwrap()
            },
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceCfg};

    #[test]
    fn reconnect_targets_only_the_requested_configured_light() {
        let state = SharedState::default();
        state.write().config.devices = vec![DeviceCfg::new("a", "A", true), DeviceCfg::new("b", "B", true)];
        let reply = execute_request(Request {cmd: "RECONNECT".into(), id: Some("a".into()), leds: None}, &state);
        assert!(reply.contains("\"ok\":true"));
        assert_eq!(state.read().renegotiate, std::collections::HashSet::from(["a".into()]));
        state.write().renegotiate.clear();
        let reply = execute_request(Request {cmd: "RECONNECT".into(), id: Some("unknown".into()), leds: None}, &state);
        assert!(reply.contains("error"));
        assert!(state.read().renegotiate.is_empty());
        execute_request(Request {cmd: "RECONNECT".into(), id: None, leds: None}, &state);
        assert_eq!(state.read().renegotiate.len(), 2);
    }

    #[test]
    fn effect_black_and_color_never_queue_cloud_power() {
        let state = SharedState::default();
        state.write().config.devices.push(DeviceCfg::new("dev", "Light", true));
        let send = |leds| execute_request(Request {cmd: "FRAME".into(), id: Some("dev".into()), leds: Some(leds)}, &state);
        // Anche il primo frame nero deve cancellare il colore precedente.
        for color in [[0; 3], [0, 255, 0], [0; 3], [255; 3]] {
            send(vec![color; 4]);
            assert_eq!(state.read().latest["dev"], [color; 4]);
            assert!(state.read().pending_power.is_empty());
        }
        execute_request(Request {cmd: "RELEASE".into(), id: Some("dev".into()), leds: None}, &state);
        assert_eq!(state.read().pending_power.get("dev"), Some(&false));
        send(vec![[0; 3]; 4]);
        assert_eq!(state.read().pending_power.get("dev"), Some(&false), "nero non annulla lo spegnimento");
        assert!(!state.read().release_after_send.contains("dev"));
    }

    #[test]
    fn sustained_black_turns_power_off_once_and_color_cancels_pending_off() {
        let state = SharedState::default();
        state.write().config.devices.push(DeviceCfg::new("dev", "Light", true));
        let send = |color| execute_request(Request {cmd:"FRAME".into(), id:Some("dev".into()), leds:Some(vec![color;4])}, &state);
        send([0;3]);
        assert!(state.read().pending_power.is_empty());
        state.write().black_since.insert("dev".into(), Instant::now()-Duration::from_secs(6));
        send([0;3]);
        assert_eq!(state.read().pending_power.get("dev"), Some(&false));
        state.write().pending_power.clear();
        send([0;3]);
        assert!(state.read().pending_power.is_empty(), "OFF non ripetuto a ogni keepalive");
        assert!(!state.read().latest.contains_key("dev"), "nessun keepalive LAN dopo OFF");
        state.write().pending_power.insert("dev".into(),false);
        send([0,255,0]);
        assert!(state.read().pending_power.is_empty());
        assert!(!state.read().black_since.contains_key("dev"));
        assert_eq!(state.read().power_requested.get("dev"),Some(&true));
    }

    fn client(port: u16) -> TcpStream {
        TcpStream::connect(("127.0.0.1", port)).unwrap()
    }

    fn roundtrip(s: &mut TcpStream, cmd: &str) -> String {
        write_frame(s, cmd).unwrap();
        let buf = read_frame(s).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn server_responds_to_hello_list_frame() {
        let mut config = Config::default();
        config.devices.push(DeviceCfg::new("dev42", "Lampada", true));
        let state = SharedState::new(config);
        let (_srv, _stop) = ControlServer::bind(state, 0).unwrap();
        let port = _srv.port;

        let mut c = client(port);
        let hello = roundtrip(&mut c, r#"{"cmd":"HELLO"}"#);
        assert!(hello.contains("\"ok\":true"));
        assert!(hello.contains("lscbridge"));

        let list = roundtrip(&mut c, r#"{"cmd":"LIST"}"#);
        assert!(list.contains("dev42"));

        let fr = roundtrip(&mut c, r#"{"cmd":"FRAME","id":"dev42","leds":[[1,2,3],[4,5,6],[7,8,9],[10,11,12]]}"#);
        assert!(fr.contains("\"ok\":true"));

        let bad = roundtrip(&mut c, r#"{"cmd":"FRAME","id":"nope","leds":[[0,0,0],[0,0,0],[0,0,0],[0,0,0]]}"#);
        assert!(bad.contains("error"));

        let rel = roundtrip(&mut c, r#"{"cmd":"RELEASE","id":"dev42"}"#);
        assert!(rel.contains("\"ok\":true"));
    }

    #[test]
    fn udp_discovers_and_updates_the_same_state_without_frame_acks() {
        let mut config = Config::default();
        config.devices.push(DeviceCfg::new("dev42", "Lampada", true));
        let state = SharedState::new(config);
        let (server, stop) = ControlServer::bind(state.clone(), 0).unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.connect(("127.0.0.1", server.port)).unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let send = |body: &str| {
            let mut packet = (body.len() as u32).to_be_bytes().to_vec();
            packet.extend_from_slice(body.as_bytes());
            socket.send(&packet).unwrap();
        };
        send(r#"{"cmd":"FRAME","id":"dev42","leds":[[0,0,255],[0,0,255],[0,0,255],[0,0,255]]}"#);
        send(r#"{"cmd":"LIST"}"#);
        let mut buf = [0u8; 4096];
        let len = socket.recv(&mut buf).unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&buf[4..len]).unwrap();
        assert_eq!(reply["devices"][0]["leds"][0], serde_json::json!([0,0,255]));
        assert_eq!(state.read().latest["dev42"], [[0,0,255];4]);
        send(r#"{"cmd":"RELEASE","id":"dev42"}"#);
        send(r#"{"cmd":"HELLO"}"#);
        socket.recv(&mut buf).unwrap();
        assert_eq!(state.read().latest["dev42"], [[0;3];4]);
        assert!(state.read().release_after_send.contains("dev42"));
        stop.store(true, Ordering::SeqCst);
    }
}
