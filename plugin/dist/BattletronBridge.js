/*
 * BattletronBridge — plugin SignalRGB per dispositivi LSC Battletron "Smart Connect".
 * Sostituisce Razer Synapse (chroma_wrapper.dll) con SignalRGB: i colori del
 * canvas vengono inviati via UDP locale al bridge Rust (lscbridge) che parla il protocollo
 * LAN originale verso i dispositivi.
 *
 * Installazione: install.ps1 genera e copia il bundle standalone, poi riavviare SignalRGB.
 */
import udp from "@SignalRGB/udp";
// Funzioni pure del plugin: framing del protocollo col bridge,
// normalizzazione colori. Nessuna dipendenza da SignalRGB (testabile in Node).
//
// Protocollo: [u32 big-endian len][json UTF-8]

function protocolEncode(obj) {
  const payload = JSON.stringify(obj).replace(/[\u007f-\uffff]/g, c => "\\u" + ("0000" + c.charCodeAt(0).toString(16)).slice(-4));
  const bytes = [];
  for (let i = 0; i < payload.length; i++) bytes.push(payload.charCodeAt(i) & 0xff);
  const header = [
    (bytes.length >>> 24) & 0xff,
    (bytes.length >>> 16) & 0xff,
    (bytes.length >>> 8) & 0xff,
    bytes.length & 0xff,
  ];
  return header.concat(bytes);
}

function protocolDecode(byteArray) {
  if (!byteArray || byteArray.length < 4) throw new Error("frame troppo corto");
  const len =
    ((byteArray[0] << 24) | (byteArray[1] << 16) | (byteArray[2] << 8) | byteArray[3]) >>> 0;
  if (byteArray.length < 4 + len) throw new Error("frame incompleto");
  let text = "";
  for (let i = 4; i < 4 + len; i++) text += String.fromCharCode(byteArray[i]);
  return JSON.parse(decodeURIComponent(escape(text)));
}

// TCP può spezzare un frame o consegnarne più di uno nello stesso evento.
function createProtocolReader() {
  let pending = [];
  return function (chunk) {
    if (chunk && !Array.isArray(chunk) && typeof chunk === "object") {
      // data è testo UTF-8: corrompe il prefisso binario. buffer conserva i byte.
      if (chunk.buffer !== undefined && chunk.length === undefined) chunk = new Uint8Array(chunk.buffer);
      else if (chunk.data !== undefined) chunk = chunk.data;
      else if (chunk.response !== undefined) chunk = chunk.response;
    }
    if (typeof chunk === "string") chunk = Array.from(chunk, c => c.charCodeAt(0) & 255);
    pending = pending.concat(Array.from(chunk));
    const messages = [];
    while (pending.length >= 4) {
      const len = ((pending[0] << 24) | (pending[1] << 16) | (pending[2] << 8) | pending[3]) >>> 0;
      if (len > 65536) throw new Error("frame troppo grande");
      if (pending.length < len + 4) break;
      messages.push(protocolDecode(pending.slice(0, len + 4)));
      pending = pending.slice(len + 4);
    }
    return messages;
  };
}

function hexToRgb(hexColor) {
  const result = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hexColor || "#000000");
  if (!result) return [0, 0, 0];
  return [parseInt(result[1], 16), parseInt(result[2], 16), parseInt(result[3], 16)];
}

// device.color può restituire [r,g,b] oppure {r,g,b} a seconda delle versioni
function normalizeColor(c) {
  if (c && typeof c.length === "number" && c.length >= 3) return [c[0] | 0, c[1] | 0, c[2] | 0];
  if (c && typeof c === "object") return [c.r | 0, c.g | 0, c.b | 0];
  return [0, 0, 0];
}


export function Name() { return "LSC Battletron Bridge"; }
export function Version() { return "1.0.0"; }
export function Publisher() { return "Personal"; }
export function DefaultComponentBrand() { return "LSC"; }
export function Type() { return "network"; }
export function SubdeviceController() { return false; }
export function ImageUrl() {
  if (typeof controller === "undefined") return "";
  const name = String(controller.name || "").toLowerCase();
  const base = "https://raw.githubusercontent.com/StargateLabs/signalrgb-lsc-battletron-bridge/main/assets/devices/";
  if (/ball/.test(name)) return base + "ball-light-3202652.png";
  if (/mousepad|mouse.?pad|pad dark/.test(name)) return base + "mousepad-xxl-3202653.png";
  if (/light.?bar|barra/.test(name)) return base + "light-bar-3207958.webp";
  if (/monitor/.test(name)) return base + "monitor-strip-3206527.png";
  if (/desk|desktop|banco/.test(name)) return base + "desk-strip-3m-3206528.png";
  return "";
}

// 4 LED sul canvas = 4 zone colore del dispositivo (come i CL2..CL5 di Chroma).
export function Size() { return [4, 1]; }
export function DefaultPosition() { return [0, 40]; }
export function DefaultScale() { return 1.0; }

const BRIDGE_DEFAULT_PORT = 21444;
const POLL_INTERVAL_MS = 3000;
const CONNECT_TIMEOUT_MS = 4000;
const BACKOFF_MIN_MS = 1000;
const BACKOFF_MAX_MS = 15000;

export function ControllableParameters() {
  return [
    {
      "property": "bridgeRecovery",
      "group": "settings",
      "label": "Recupero LAN (torna su Riposo per ripetere)",
      "type": "combobox",
      "values": ["Riposo", "Riconnetti questa luce", "Riconnetti tutte le luci"],
      "default": "Riposo"
    },
    {
      "property": "bridgeHost",
      "group": "settings",
      "label": "Bridge host",
      "type": "textfield",
      "default": "127.0.0.1"
    },
    {
      "property": "bridgePort",
      "group": "settings",
      "label": "Bridge porta UDP locale",
      "type": "number",
      "min": "1",
      "max": "65535",
      "default": String(BRIDGE_DEFAULT_PORT)
    },
    {
      "property": "bridgeFrameRate", "group": "settings", "label": "Aggiornamenti massimi al secondo",
      "description": "Limite del plugin; le Ball mantengono il limite LAN dedicato. Aumentare non elimina la latenza Wi-Fi.",
      "type": "combobox", "values": ["5", "10", "15", "20", "30"], "default": "30"
    },
    {
      "property": "lightingMode",
      "group": "lighting",
      "label": "Sorgente colore",
      "type": "combobox",
      "values": ["Canvas", "Colore fisso"],
      "default": "Canvas"
    },
    {
      "property": "forcedColor",
      "group": "lighting",
      "label": "Colore di prova",
      "type": "color",
      "default": "#00ff00"
    },
    {
      "property": "shutdownColor",
      "group": "lighting",
      "label": "Colore allo Shutdown",
      "type": "color",
      "min": "0",
      "max": "360",
      "default": "#000000"
    },
    {
      "property": "onShutdown",
      "group": "lighting",
      "label": "Alla chiusura di SignalRGB",
      "type": "combobox",
      "values": ["Spegni luci", "Colore fisso"],
      "default": "Spegni luci"
    }
  ];
}

export function Validate() { return true; }

// stato per il render loop di QUESTO controller
let socket = null;
let socketEndpoint = "";
let bridgeId = "unknown";
let lastFrameTime = 0;
let lastSentAt = 0;
let lastSentColors = "";
let connecting = false;
let connectDeadline = 0;
let backoffMs = BACKOFF_MIN_MS;
let nextTryAt = 0;
let framesSent = 0;
let connected = false;
let pendingRecovery = null;

export function onbridgeRecoveryChanged() {
  if (typeof bridgeRecovery === "undefined" || bridgeRecovery === "Riposo") return;
  pendingRecovery = bridgeRecovery === "Riconnetti tutte le luci"
    ? { cmd: "RECONNECT" } : { cmd: "RECONNECT", id: bridgeId };
}

function log(msg) {
  device.log(msg, { toFile: true });
}

function endpointOf() {
  const host = controller.ip || (controller.device && controller.device.host) || bridgeHost;
  const port = controller.port || (controller.device && controller.device.port) || bridgePort;
  return { host: host || "127.0.0.1", port: parseInt(port) || BRIDGE_DEFAULT_PORT };
}

// This UDP runtime logs unsupported aliases instead of throwing an exception.
function onSafe(sock, names, handler) {
  const list = (Array.isArray(names) ? names : [names])
    .filter(name => ["connection", "close", "error", "message"].includes(name));
  let registered = false;
  for (const n of list) {
    try { sock.on(n, handler); registered = true; } catch (e) { /* prova il prossimo */ }
  }
  return registered;
}

function closeSocket(s) {
  try { if (s) s.disconnect(); } catch (e) { /* API non disponibile */ }
  try { if (s) s.close(); } catch (e) { /* già chiuso */ }
}

function connect() {
  if (connected && socket && socket.state === socket.ConnectedState) return true;
  const now = Date.now();
  if (connecting && now < connectDeadline) return false; // attesa timeout
  if (now < nextTryAt) return false; // backoff
  connecting = true;
  connectDeadline = now + CONNECT_TIMEOUT_MS;
  const ep = endpointOf();
  const endpoint = ep.host + ":" + ep.port;
  // Riusa il QObject nativo anche nei tentativi falliti: massimo un socket
  // per controller, senza accumulare oggetti e callback a ogni riconnessione.
  if (socket && socketEndpoint === endpoint) {
    try {
      if (socket.state === socket.ConnectedState) {
        connected = true;
        connecting = false;
        return true;
      }
      socket.connect(ep.host, ep.port);
    } catch (e) {
      connecting = false;
      nextTryAt = now + backoffMs;
    }
    return false;
  }
  if (socket) { const previous = socket; socket = null; closeSocket(previous); }
  try {
    const s = udp.createSocket();
    socket = s;
    socketEndpoint = endpoint;
    connected = false;
    onSafe(s, ["connected", "connection"], () => {
      if (socket !== s) return;
      if (connected) return;
      connecting = false;
      connected = true;
      backoffMs = BACKOFF_MIN_MS;
      nextTryAt = 0;
      log("Socket UDP locale pronto per " + ep.host + ":" + ep.port);
      try { s.send(protocolEncode({ cmd: "HELLO", ver: 1 })); } catch (e) {}
    });
    onSafe(s, ["disconnected", "close"], () => {
      if (socket !== s) return;
      connected = false;
      connecting = false;
      nextTryAt = Date.now() + backoffMs;
      backoffMs = Math.min(backoffMs * 2, BACKOFF_MAX_MS);
      log("Bridge disconnesso, riprovo fra " + Math.round(backoffMs / 1000) + "s");
    });
    onSafe(s, ["error", "err"], (err, message) => {
      if (socket !== s) return;
      connected = false;
      connecting = false;
      nextTryAt = Date.now() + backoffMs;
      backoffMs = Math.min(backoffMs * 2, BACKOFF_MAX_MS);
      // Il callback nativo può arrivare da un altro thread: niente notifiche UI.
      try { log("Errore bridge: " + (message || err)); } catch (e) {}
    });
    onSafe(s, ["message"], () => { /* ack dal bridge: nessuna azione */ });
    s.bind(0);
    s.connect(ep.host, ep.port);
  } catch (e) {
    connecting = false;
    connected = false;
    nextTryAt = Date.now() + backoffMs;
    backoffMs = Math.min(backoffMs * 2, BACKOFF_MAX_MS);
    try { log("Connessione fallita: " + e.message); } catch (e2) {}
  }
  return false;
}

export function Initialize() {
  if (typeof device.setFrameRateTarget === "function") device.setFrameRateTarget(30);
  // dati persistiti del controller (id, nome, host/port)
  bridgeId = controller.id || (controller.device && controller.device.id) || "unknown";
  const name = controller.name || (controller.device && controller.device.name) || ("LSC " + bridgeId);

  device.setName(name);
  device.setImageFromBase64("");
  device.setSize([4, 1]);
  device.setControllableLeds(["Led 1", "Led 2", "Led 3", "Led 4"], [[0, 0], [1, 0], [2, 0], [3, 0]]);
  // persisti host/porta per il discovery service
  try {
    service.saveSetting("battletron", "host", bridgeHost || "127.0.0.1");
    service.saveSetting("battletron", "port", String(parseInt(bridgePort) || BRIDGE_DEFAULT_PORT));
  } catch (e) { /* il service non è sempre disponibile in Initialize */ }
  log("Inizializzato controller " + bridgeId + " (" + name + ")");
  nextTryAt = 0;
  connect();
}

export function Render() {
  if (bridgeId === "unknown") return;
  if (!connected || !socket || socket.state !== socket.ConnectedState) {
    connect();
    return;
  }
  if (pendingRecovery) {
    try {
      if (socket.send(protocolEncode(pendingRecovery)) === -1) return;
      log("Riconnessione LAN richiesta: " + (pendingRecovery.id || "tutte le luci") +
        "; servizio locale richiesto su porta 21444");
      pendingRecovery = null;
    } catch (e) { log("Riconnessione LAN non inviata: " + e.message); return; }
  }
  const now = Date.now();
  let rate = 30;
  try { if ([5, 10, 15, 20, 30].includes(Number(bridgeFrameRate))) rate = Number(bridgeFrameRate); } catch (_) {}
  const delay = Math.floor(1000 / rate);
  if (delay > 0 && now - lastFrameTime < delay) return;
  const frameGap = lastFrameTime ? now - lastFrameTime : 0;
  lastFrameTime = now;

  const leds = [];
  const fixed = lightingMode === "Colore fisso" ? normalizeColor(device.createColorArray(forcedColor, 1, "Inline")) : null;
  for (let i = 0; i < 4; i++) {
    leds.push(fixed || normalizeColor(device.color(i, 0)));
  }
  const colors = JSON.stringify(leds);
  // Keepalive sotto il watchdog di due secondi, senza duplicare ogni frame statico.
  if (colors === lastSentColors && now - lastSentAt < 500) return;
  try {
    const sent = socket.send(protocolEncode({ cmd: "FRAME", id: bridgeId, leds: leds }));
    if (sent === -1) throw new Error("Invio UDP fallito");
    if (sent > 0) {
      lastSentColors = colors;
      lastSentAt = now;
      framesSent++;
      if (framesSent === 1 || framesSent % 300 === 0) {
        const brightness = typeof device.getBrightness === "function" ? device.getBrightness() : "n/d";
        log("Frame inviati: " + framesSent + "; luminosita=" + brightness +
          "; intervallo_ms=" + frameGap + "; limite_ms=" + delay +
          "; sorgente=" + lightingMode + "; RGB=" + JSON.stringify(leds));
      }
    }
  } catch (e) {
    connected = false;
    nextTryAt = Date.now() + backoffMs;
    backoffMs = Math.min(backoffMs * 2, BACKOFF_MAX_MS);
  }
}

export function Shutdown(SystemSuspending) {
  try {
    if (onShutdown === "Colore fisso" && socket && connected) {
      const col = normalizeColor(device.createColorArray(shutdownColor, 1, "Inline"));
      socket.send(protocolEncode({ cmd: "HOLD", id: bridgeId, leds: [col, col, col, col] }));
    }
    else if (socket && connected) {
      socket.send(protocolEncode({ cmd: "RELEASE", id: bridgeId }));
    }
  } catch (e) {
    log("Shutdown: " + e.message);
  }
  connected = false;
  const previous = socket;
  socket = null;
  closeSocket(previous);
  log("Plugin spento");
}

// callback impostazioni: persiste host/porta per il discovery
export function onbridgeHostChanged() {
  try { service.saveSetting("battletron", "host", bridgeHost || "127.0.0.1"); } catch (e) {}
  nextTryAt = 0;
}
export function onbridgePortChanged() {
  try { service.saveSetting("battletron", "port", String(parseInt(bridgePort) || BRIDGE_DEFAULT_PORT)); } catch (e) {}
  nextTryAt = 0;
}

/* ------------------------------------------------------------------ *
 * DiscoveryService: interroga il bridge Rust e crea/aggiorna i
 * controller in SignalRGB (addon "network" = dispositivo virtuale).
 * ------------------------------------------------------------------ */
export function DiscoveryService() {
  this.IconUrl = "";
  this.lastPoll = 0;
  this.controllers = {};
  this.known = {};
  this.pendingDevices = null;
  this.pollFinish = null;
  this.pollDeadline = 0;
  this.pollSocket = null;
  this.pollEndpoint = "";
  this.pollReady = false;
  this.status = "Ricerca del bridge locale...";
  this.deviceStates = {};
  this.pollRequest = {cmd: "LIST"};

  this.Initialize = function () {
    service.log("BattletronBridge discovery avviato");
    this.lastPoll = 0;
  };

  this.poll = function (request) {
    if (this.pollFinish) return;
    this.pollRequest = request || {cmd: "LIST"};
    const ep = { host: "127.0.0.1", port: BRIDGE_DEFAULT_PORT };
    try {
      ep.host = service.getSetting("battletron", "host") || ep.host;
      ep.port = parseInt(service.getSetting("battletron", "port")) || ep.port;
    } catch (e) { /* usa endpoint locale predefinito */ }
    const endpoint = ep.host + ":" + ep.port;
    this.pollFinish = () => { this.pollFinish = null; };
    this.pollDeadline = Date.now() + CONNECT_TIMEOUT_MS;
    try {
      if (this.pollSocket && this.pollEndpoint !== endpoint) {
        const previous = this.pollSocket;
        this.pollSocket = null;
        closeSocket(previous);
      }
      if (this.pollSocket) {
        if (this.pollReady || this.pollSocket.state === this.pollSocket.ConnectedState) {
          this.pollSocket.send(protocolEncode(this.pollRequest));
        } else {
          this.pollSocket.connect(ep.host, ep.port);
        }
        return;
      }
      const sock = udp.createSocket();
      this.pollSocket = sock;
      this.pollEndpoint = endpoint;
      this.pollReady = false;
      let readMessages = createProtocolReader();
      onSafe(sock, ["connected", "connection"], () => {
        if (this.pollSocket !== sock || this.pollReady) return;
        this.pollReady = true;
        sock.send(protocolEncode(this.pollRequest));
      });
      onSafe(sock, "message", data => {
        if (this.pollSocket !== sock) return;
        // La modifica del modello resta nell'Update, fuori dal callback nativo.
        try {
          const messages = readMessages(data);
          if (!messages.length) return;
          const parsed = messages[0];
          if (this.pollRequest.cmd === "RECONNECT") {
            this.status = parsed.ok ? "Riconnessione LAN richiesta; aggiornamento stato in corso" : "Riconnessione non riuscita: " + (parsed.error || "richiesta rifiutata");
            this.lastPoll = 0;
            if (this.pollFinish) this.pollFinish();
            return;
          }
          if (!Array.isArray(parsed.devices)) throw new Error("LIST senza devices");
          this.deviceStates = {};
          for (const d of parsed.devices) this.deviceStates[d.id] = d.online === true;
          this.pendingDevices = parsed.devices.filter(d => d.online !== false);
          this.status = this.pendingDevices.length + " luci online rilevate dal bridge";
        } catch (e) {
          this.status = "Risposta bridge non valida: " + e.message;
          readMessages = createProtocolReader();
          service.log(this.status);
        }
        if (this.pollFinish) this.pollFinish();
      });
      onSafe(sock, ["error", "err", "disconnected", "close"], () => {
        if (this.pollSocket !== sock) return;
        this.pollReady = false;
        this.status = "Bridge locale non raggiungibile";
        if (this.pollFinish) this.pollFinish();
      });
      sock.bind(0);
      sock.connect(ep.host, ep.port);
    } catch (e) {
      this.status = "Errore discovery: " + e.message;
      service.log(this.status);
      if (this.pollFinish) this.pollFinish();
    }
  };

  this.Shutdown = function () {
    const sock = this.pollSocket;
    this.pollSocket = null;
    this.pollFinish = null;
    closeSocket(sock);
  };

  this.applyDevices = function (devices) {
    for (const d of devices) {
      if (!this.known[d.id]) {
        // guard anti-duplicati: SignalRGB persiste i controller tra riavvi
        let exists = false;
        try { exists = service.hasController(d.id); } catch (e) { exists = false; }
        if (exists) {
          try {
            const cached = service.getController(d.id);
            if (cached) {
              cached.ip = service.getSetting("battletron", "host") || "127.0.0.1";
              cached.port = parseInt(service.getSetting("battletron", "port")) || BRIDGE_DEFAULT_PORT;
              if (typeof cached.update !== "function") cached.update = function () {};
              this.controllers[d.id] = cached;
              service.announceController(cached);
            }
          } catch (e) { service.log("ripristino controller: " + e.message); }
          this.known[d.id] = d.name || "";
          continue;
        }
        const ctl = {
          id: d.id,
          name: d.name || ("LSC " + d.id),
          // Il runtime richiama update() sui controller anche senza nuovi dati.
          update: function () {},
          ip: service.getSetting("battletron", "host") || "127.0.0.1",
          port: parseInt(service.getSetting("battletron", "port")) || BRIDGE_DEFAULT_PORT
        };
        this.controllers[d.id] = ctl;
        service.addController(ctl);
        service.announceController(ctl);
        service.log("Controller annunciato: " + d.id);
        this.known[d.id] = d.name || "";
      } else if (this.controllers[d.id] && this.controllers[d.id].name !== d.name) {
        this.controllers[d.id].name = d.name;
        service.updateController(this.controllers[d.id]);
        service.announceController(this.controllers[d.id]);
        this.known[d.id] = d.name || "";
      }
    }
    // LIST contiene soltanto i dispositivi online: un timeout non è una cancellazione.
    // Il runtime può ancora usare il controller; la rimozione spetta a Delete().
  };

  this.Delete = function (id) {
    if (this.controllers[id]) {
      service.removeController(this.controllers[id]);
      delete this.controllers[id];
    }
    delete this.known[id];
  };

  this.Reconnect = function (id) {
    if (this.pollFinish) {
      this.status = "Attendere il completamento della richiesta in corso";
      return;
    }
    this.status = id ? "Riconnessione del dispositivo..." : "Riconnessione di tutti i dispositivi...";
    this.poll(id ? {cmd: "RECONNECT", id: id} : {cmd: "RECONNECT"});
  };

  this.IsOnline = function (id) { return this.deviceStates[id] === true; };

  this.Update = function (force) {
    const now = Date.now();
    if (this.pollFinish && now >= this.pollDeadline) {
      this.status = "Nessuna risposta dal bridge locale";
      this.pollFinish();
    }
    if (force || now - this.lastPoll > POLL_INTERVAL_MS) {
      this.lastPoll = now;
      this.poll();
    }
    // applica i risultati in arrivo (fuori dal callback TCP)
    if (this.pendingDevices) {
      const d = this.pendingDevices;
      this.pendingDevices = null;
      this.applyDevices(d);
    }
  };
}
