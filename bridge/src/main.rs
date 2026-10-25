// lscbridge â€” bridge LSC Battletron per SignalRGB.
// Reimplementa il protocollo di rete dell'app ufficiale 2.0.17 con SignalRGB
// al posto di Razer Synapse.
use lscbridge::config::Config;
use lscbridge::proto::udp::UdpRuntime;
use lscbridge::server::ControlServer;
use lscbridge::state::SharedState;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn tag(&self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

static LOG_FILE: std::sync::OnceLock<std::sync::Mutex<std::fs::File>> = std::sync::OnceLock::new();

fn init_log() {
    let dir = Config::dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("bridge.log"))
    {
        let _ = LOG_FILE.set(std::sync::Mutex::new(f));
    }
}

fn log(level: LogLevel, msg: &str) {
    let line = format!("{} [{}] {}\n", chrono_like_now(), level.tag(), msg);
    if let Some(f) = LOG_FILE.get() {
        if let Ok(mut f) = f.lock() {
            let _ = f.write_all(line.as_bytes());
        }
    }
    eprint!("{}", line);
}

fn chrono_like_now() -> String {
    // timestamp locale senza dipendenze
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, mi, s) = civil_from_unix(now as i64);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, mi, s)
}

fn civil_from_unix(ts: i64) -> (i64, u64, u64, u64, u64, u64) {
    let days = ts.div_euclid(86400);
    let secs = ts.rem_euclid(86400);
    let (h, mi, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    // algoritmo civile standard (Howard Hinnant)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u64, d as u64, h as u64, mi as u64, s as u64)
}

fn usage() -> ! {
    eprintln!(
        "lscbridge {} â€” bridge LSC Battletron â†’ SignalRGB\n\n\
         Uso:\n  \
         lscbridge run [--port N] [--fps N]   avvia il bridge (default)\n  \
         lscbridge key add <devId> <localKey> aggiunge un dispositivo (localKey manuale)\n  \
         lscbridge key list                  elenca dispositivi e localKey\n  \
         lscbridge key del <devId>          rimuove un dispositivo\n  \
         lscbridge status                   stato: config, porte, dispositivi\n  \
         lscbridge login                    login cloud Tuya (QR) per ottenere le localKey\n  \
         lscbridge devices                  elenca i dispositivi dal cloud (dopo login)",
        env!("CARGO_PKG_VERSION")
    );
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("run");
    if cmd == "-h" || cmd == "--help" || cmd == "help" {
        usage();
    }
    init_log();
    let rest = args.get(1..).unwrap_or(&[]);
    match cmd {
        "run" => cmd_run(rest),
        "key" => cmd_key(rest),
        "login" => cmd_login(rest),
        "devices" => cmd_devices(rest),
        "status" => cmd_status(),
        "reconnect" => cmd_reconnect(rest),
        _ => usage(),
    }
}

fn load_config() -> Config {
    Config::load().unwrap_or_else(|e| {
        log(LogLevel::Error, &format!("lettura config {}: {}", Config::path().display(), e));
        std::process::exit(1);
    })
}

fn save_config(config: &Config) {
    config.save().unwrap_or_else(|e| {
        log(LogLevel::Error, &format!("salvataggio config: {}", e));
        std::process::exit(1);
    });
}

fn cmd_run(rest: &[String]) {
    let mut config = load_config();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--port" => {
                i += 1;
                if let Some(p) = rest.get(i) {
                    config.bridge_port = p.parse().unwrap_or(config.bridge_port);
                }
            }
            "--fps" => {
                i += 1;
                if let Some(f) = rest.get(i) {
                    config.fps = f.parse().unwrap_or(config.fps);
                }
            }
            _ => {}
        }
        i += 1;
    }
    save_config(&config);
    let state = SharedState::new(config);

    log(LogLevel::Info, &format!("lscbridge {} â€” avvio", env!("CARGO_PKG_VERSION")));
    log(
        LogLevel::Info,
        &format!(
            "{} dispositivi in configurazione",
            state.read().config.devices.len()
        ),
    );

    let port = state.read().config.bridge_port;
    let fps = state.read().config.fps;
    let (_srv, stop) = ControlServer::bind(state.clone(), port)
        .unwrap_or_else(|e| {
            log(LogLevel::Error, &format!("control server non avviato: {}", e));
            std::process::exit(1);
        });
    log(
        LogLevel::Info,
        &format!("control server su 127.0.0.1:{}", _srv.port),
    );

    let mut rt = UdpRuntime::new(state.clone(), fps);
    log(
        LogLevel::Info,
        &format!("mate_id principale: {}", hex::encode(rt.main_mate_id)),
    );

    let stop2 = stop.clone();
    ctrlc(stop2);

    let t = std::thread::spawn(move || rt.run_mut());
    let _ = t.join();

    stop.store(true, Ordering::SeqCst);
    log(LogLevel::Info, "arresto");
}

fn cmd_key(rest: &[String]) {
    let mut config = load_config();
    match rest.first().map(|s| s.as_str()) {
        Some("add") => {
            let dev_id = rest.get(1).cloned().unwrap_or_else(|| usage());
            let local_key = rest.get(2).cloned().unwrap_or_else(|| usage());
            let simple = rest.iter().any(|a| a == "--simple");
            let exists = config.device(&dev_id).is_some();
            if exists {
                let d = config.device_mut(&dev_id).unwrap();
                d.local_key = Some(local_key);
                if simple {
                    d.rgbic = false;
                }
            } else {
                let mut d = lscbridge::config::DeviceCfg::new(dev_id.clone(), dev_id.clone(), !simple);
                d.local_key = Some(local_key);
                config.devices.push(d);
            }
            config.save().unwrap_or_else(|e| {
                log(LogLevel::Error, &format!("salvataggio config: {}", e));
                std::process::exit(1);
            });
            println!(
                "dispositivo {} salvato (tipo: {})",
                dev_id,
                if simple { "semplice (dp24)" } else { "RGBIC (dp61)" }
            );
        }
        Some("del") => {
            let dev_id = rest.get(1).cloned().unwrap_or_else(|| usage());
            config.devices.retain(|d| d.dev_id != dev_id);
            save_config(&config);
            println!("dispositivo {} rimosso", dev_id);
        }
        Some("list") | None => {
            for d in &config.devices {
                println!(
                    "{}\t{}\trgbic={}\tkey={}",
                    d.dev_id,
                    d.name,
                    d.rgbic,
                    d.local_key.as_deref().unwrap_or("-")
                );
            }
        }
        _ => usage(),
    }
}

/// Login cloud: QR nel terminale, poll, salva sid/ecode in config.
fn cmd_login(rest: &[String]) {
    let mut config = load_config();
    if let Some(region) = rest.iter().find(|s| !s.starts_with("--")) {
        config.region = region.clone();
    }
    let mut client = lscbridge::cloud::TuyaClient::new(&config.region);
    let login = match client.qr_login_start() {
        Ok(l) => l,
        Err(e) => {
            log(LogLevel::Error, &format!("qr.token.create fallito: {}", e));
            std::process::exit(1);
        }
    };
    println!("Scansiona con LSC Smart Connect sul telefono: Io/Profilo > scanner in alto a destra.");
    println!("(il QR compare qui sotto; la pagina si aggiorna dopo la scansione)\n");
    if let Ok(html) = lscbridge::cloud::login::qr_html(&login.qr_payload) {
        let path = Config::dir().join("login.html");
        if std::fs::write(&path, html).is_ok() {
            println!("QR nel browser: {}", path.display());
            if rest.iter().any(|s| s == "--open") {
                let _ = std::process::Command::new("explorer.exe").arg(&path).spawn();
            }
        }
    }
    match qr2term::print_qr(&login.qr_payload) {
        Ok(_) => {}
        Err(_) => println!("payload: {}", login.qr_payload),
    }
    for _ in 0..60 {
        std::thread::sleep(Duration::from_secs(2));
        match client.qr_login_poll(&login.token) {
            Ok(Some((sid, ecode))) => {
                config.session = Some(lscbridge::config::SessionCfg { sid, ecode });
                config.region = client.region.clone();
                save_config(&config);
                let page = Config::dir().join("login.html");
                if let Ok(html) = std::fs::read_to_string(&page) {
                    let _ = std::fs::write(&page, html.replace("IN ATTESA DELLA TUA SCANSIONE", "LOGIN RIUSCITO · CONTINUA NEL BRIDGE"));
                }
                log(LogLevel::Info, "login riuscito, sessione salvata");
                return;
            }
            Ok(None) => continue,
            Err(e) => {
                log(LogLevel::Warn, &format!("poll: {}", e));
            }
        }
    }
    log(LogLevel::Error, "login scaduto, riprova");
    std::process::exit(1);
}

/// Sincronizza i dispositivi dal cloud (dopo login) nella config locale.
fn cmd_devices(_rest: &[String]) {
    let mut config = load_config();
    let mut client = lscbridge::cloud::TuyaClient::new(&config.region);
    if let Some(s) = &config.session {
        client.sid = Some(s.sid.clone());
        client.ecode = Some(s.ecode.clone());
    }
    let gid = match client.first_home_id() {
        Ok(g) => g,
        Err(e) => {
            log(LogLevel::Error, &format!("location.list fallito: {}", e));
            std::process::exit(1);
        }
    };
    let devices = match client.devices(&gid) {
        Ok(d) => d,
        Err(e) => {
            log(LogLevel::Error, &format!("device.list fallito: {}", e));
            std::process::exit(1);
        }
    };
    for cd in &devices {
        let existing = config.device_mut(&cd.dev_id);
        match existing {
            Some(d) => {
                d.name = cd.name.clone();
                d.rgbic = cd.rgbic;
                if !cd.local_key.is_empty() {
                    d.local_key = Some(cd.local_key.clone());
                }
            }
            None => {
                let mut d = lscbridge::config::DeviceCfg::new(cd.dev_id.clone(), cd.name.clone(), cd.rgbic);
                d.local_key = if cd.local_key.is_empty() { None } else { Some(cd.local_key.clone()) };
                config.devices.push(d);
            }
        }
        println!(
            "{}\t{}\trgbic={}\tkey={}",
            cd.dev_id,
            cd.name,
            cd.rgbic,
            if cd.local_key.is_empty() { "-" } else { "presente" }
        );
    }
    save_config(&config);
    log(LogLevel::Info, &format!("{} dispositivi salvati in config", devices.len()));
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], config.bridge_port));
    if let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(250)) {
        let body = b"{\"cmd\":\"RELOAD\"}";
        let result = stream.write_all(&(body.len() as u32).to_be_bytes())
            .and_then(|_| stream.write_all(body));
        if let Err(e) = result {
            log(LogLevel::Warn, &format!("ricaricamento bridge: {}", e));
        }
    }
}

fn cmd_reconnect(args: &[String]) {
    use std::io::Read;
    if args.len() > 1 { usage(); }
    let config = load_config();
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], config.bridge_port));
        let mut socket = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
        socket.set_read_timeout(Some(Duration::from_secs(2)))?;
        let body = serde_json::to_vec(&serde_json::json!({"cmd":"RECONNECT","id":args.first()}))?;
        socket.write_all(&(body.len() as u32).to_be_bytes())?;
        socket.write_all(&body)?;
        let mut header = [0; 4];
        socket.read_exact(&mut header)?;
        let len = u32::from_be_bytes(header) as usize;
        if len > 65536 { return Err("risposta bridge troppo grande".into()); }
        let mut reply = vec![0; len];
        socket.read_exact(&mut reply)?;
        let reply: serde_json::Value = serde_json::from_slice(&reply)?;
        if let Some(error) = reply["error"].as_str() { return Err(error.to_string().into()); }
        if reply["ok"] != true { return Err("risposta bridge non valida".into()); }
        Ok(())
    })();
    if let Err(error) = result {
        log(LogLevel::Error, &format!("riconnessione: {}", error));
        std::process::exit(1);
    }
    println!("Nuova negoziazione LAN richiesta; attendere l'autenticazione.");
}

fn cmd_status() {
    let config = load_config();
    println!("config: {}", Config::path().display());
    println!("porta control server: {}", config.bridge_port);
    println!("fps: {}", config.fps);
    println!("regione cloud: {}", config.region);
    println!("dispositivi: {}", config.devices.len());
    for d in &config.devices {
        println!(
            "  {} [{}] rgbic={} localKey={}",
            d.dev_id,
            d.name,
            d.rgbic,
            if d.local_key.is_some() { "presente" } else { "MANCANTE" }
        );
    }
}

fn ctrlc(stop: Arc<AtomicBool>) {
    // minimale: Ctrl-C chiude il processo; il flag resta per UPDATE futuri
    let _ = stop;
    let _ = Duration::from_millis(1);
}

