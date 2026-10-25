// Test del plugin: sintassi, export e framing del protocollo (eseguito davvero).
// Eseguire: node plugin/test/frame.test.mjs
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { protocolEncode, protocolDecode, createProtocolReader, hexToRgb, normalizeColor } from "../lib/framing.js";
import vm from "node:vm";

const here = dirname(fileURLToPath(import.meta.url));
const pluginSrc = readFileSync(join(here, "..", "BattletronBridge.js"), "utf8");

let pass = 0, fail = 0;
function check(name, cond) {
  if (cond) { pass++; console.log("ok -", name); }
  else { fail++; console.log("FAIL -", name); }
}

const unicode = { devices: [{ id: "lamp1", name: "Caffè 灯 💡" }] };
const utf8 = Buffer.from(JSON.stringify(unicode), "utf8");
const wire = [0, 0, utf8.length >>> 8, utf8.length & 255, ...utf8];
const reader = createProtocolReader();
check("TCP header frammentato", reader(wire.slice(0, 2)).length === 0);
check("TCP body frammentato", reader(wire.slice(2, 9)).length === 0);
check("TCP frame uniti e UTF-8", reader(wire.slice(9).concat(protocolEncode({ok:true})))[0].devices[0].name === unicode.devices[0].name);
check("encoding Unicode interoperabile", JSON.parse(Buffer.from(protocolEncode(unicode).slice(4)).toString("utf8")).devices[0].name === unicode.devices[0].name);
check("eventi socket con envelope data", createProtocolReader()({data: protocolEncode({ok:true})})[0].ok);
check("eventi socket con envelope response", createProtocolReader()({response: String.fromCharCode(...protocolEncode({ok:true}))})[0].ok);
const nativeFrame = protocolEncode({devices: [{name: "x".repeat(400)}]});
check("socket nativo usa buffer binario prima del testo corrotto", createProtocolReader()({
  buffer: Uint8Array.from(nativeFrame).buffer,
  data: Buffer.from(nativeFrame).toString("utf8")
})[0].devices[0].name.length === 400);
let oversized = false;
try { createProtocolReader()([0, 1, 0, 1]); } catch { oversized = true; }
check("TCP rifiuta frame oltre 64 KiB", oversized);

// Esegue DiscoveryService reale con socket simulato: niente copie del suo algoritmo.
const sockets = [], announced = [], removed = [];
const sandbox = {
  protocolEncode, createProtocolReader, hexToRgb, normalizeColor,
  udp: { createSocket() {
    const s = { handlers: {}, packets: [], state: 1, ConnectedState: 1, on(n, cb) { this.handlers[n] = cb; },
      bind() {}, connect() { this.handlers.connection(); }, send(bytes) { this.sent = true; this.packets.push(bytes); return bytes.length; }, close() { this.closed = true; } };
    sockets.push(s); return s;
  } },
  service: { log() {}, getSetting(_group, key) { return key === "port" ? "21445" : "127.0.0.1"; },
    hasController() { return false; }, addController() {}, announceController(c) { announced.push(c); },
    updateController() {}, removeController(c) { removed.push(c.id); } }
};
vm.createContext(sandbox);
vm.runInContext(pluginSrc.replace(/^import .*;\r?$/gm, "").replace(/export function /g, "function ") + "\nthis.discovery = new DiscoveryService();", sandbox);
sandbox.discovery.Initialize();
sandbox.discovery.poll();
check("discovery invia LIST su evento connection", sockets[0].sent === true);
sockets[0].handlers.message(wire.slice(0, 7));
check("discovery attende frame completo", !sockets[0].closed);
sockets[0].handlers.message(wire.slice(7));
sandbox.discovery.Update(false);
check("discovery annuncia dispositivo Unicode", announced[0]?.name === unicode.devices[0].name);
check("discovery mantiene endpoint nativo", announced[0]?.ip === "127.0.0.1" && announced[0]?.port === 21445);
check("controller espone update richiesto dal runtime nativo", typeof announced[0]?.update === "function");
const discoverySocketCount = sockets.length;
sockets[0].handlers.message(wire);
sandbox.discovery.poll();
check("discovery riusa socket e callback fra i poll", sockets.length === discoverySocketCount);
sandbox.discovery.applyDevices([]);
check("assenza temporanea non distrugge controller in uso", removed.length === 0 && sandbox.discovery.controllers.lamp1 === announced[0]);
sandbox.discovery.applyDevices(unicode.devices);
check("riconnessione mantiene lo stesso controller", announced.length === 1 && sandbox.discovery.controllers.lamp1 === announced[0]);
sandbox.discovery.Delete("lamp1");
check("cancellazione esplicita rimuove il controller", removed[0] === "lamp1" && !sandbox.discovery.controllers.lamp1);
sockets[0].handlers.message(protocolEncode({devices:[]}));
sandbox.discovery.Reconnect('lamp1');
check("riconnessione mirata usa lo stesso socket", sockets.length === discoverySocketCount &&
  JSON.stringify(protocolDecode(sockets[0].packets.at(-1))) === JSON.stringify({cmd:'RECONNECT',id:'lamp1'}));
const reconnectPackets=sockets[0].packets.length;
sandbox.discovery.Reconnect('lamp1');
check("clic ripetuti non accodano riconnessioni", sockets[0].packets.length===reconnectPackets);
sockets[0].handlers.message(protocolEncode({ok:true}));
check("ACK riconnessione non viene interpretato come LIST", sandbox.discovery.status.includes('richiesta') && !sandbox.discovery.pollFinish);
sandbox.discovery.Reconnect('');
check("riconnessione globale omette id", protocolDecode(sockets[0].packets.at(-1)).cmd==='RECONNECT' && !('id' in protocolDecode(sockets[0].packets.at(-1))));
sockets[0].handlers.message(protocolEncode({error:'device sconosciuto'}));
check("errore riconnessione viene mostrato", sandbox.discovery.status.includes('device sconosciuto'));
sandbox.discovery.poll();
sockets[0].handlers.message(protocolEncode({devices:[{id:'lamp1',name:'Lampada',online:false}]}));
check("LIST offline aggiorna lo stato senza cancellare controller", !sandbox.discovery.IsOnline('lamp1'));

// 1. sintassi valida
const c1 = spawnSync(process.execPath, ["--check", join(here, "..", "BattletronBridge.js")]);
check("sintassi BattletronBridge.js valida", c1.status === 0);
const c2 = spawnSync(process.execPath, ["--check", join(here, "..", "lib", "framing.js")]);
check("sintassi lib/framing.js valida", c2.status === 0);

// 2. export presenti
for (const fn of [
  "Name", "Version", "Publisher", "Type", "Size", "DefaultPosition", "DefaultScale",
  "ControllableParameters", "Initialize", "Render", "Shutdown", "Validate", "DiscoveryService",
  "SubdeviceController", "onbridgeHostChanged", "onbridgePortChanged",
]) {
  check(`export ${fn} presente`, new RegExp(`export function ${fn}\\s*\\(`).test(pluginSrc));
}

check('Type() ritorna "network"', /export function Type\(\)\s*\{\s*return "network"/.test(pluginSrc));
check("Size è [4,1]", /export function Size\(\)\s*\{\s*return \[4, 1\]/.test(pluginSrc));
check("controlli usano tipi supportati da SignalRGB", sandbox.ControllableParameters().every(p => ["boolean", "number", "hue", "color", "combobox", "textfield"].includes(p.type)));
check("usa modulo UDP nativo SignalRGB", /import udp from "@SignalRGB\/udp"/.test(pluginSrc));
check("framing condiviso con lib/framing.js", /from "\.\/lib\/framing\.js"/.test(pluginSrc));
check("discovery: addController", /service\.addController/.test(pluginSrc));
check("discovery: announceController", /service\.announceController/.test(pluginSrc));
check("discovery: removeController", /service\.removeController/.test(pluginSrc));
check("discovery: guardia hasController", /service\.hasController/.test(pluginSrc));
check("reconnect con backoff", /backoffMs/.test(pluginSrc) && /BACKOFF_MAX_MS/.test(pluginSrc));
check("eventi registrati con fallback", /onSafe\(/.test(pluginSrc));

// 3. framing eseguito (round-trip col parser del bridge)
const frameBytes = protocolEncode({ cmd: "FRAME", id: "6c8e0d9f3a1b2c4d5e", leds: [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]] });
check("prefix u32 BE corretto", frameBytes[0] === 0 && frameBytes[1] === 0 && frameBytes[2] === 0 && frameBytes[3] > 0);
const decoded = protocolDecode(frameBytes);
check("round-trip cmd", decoded.cmd === "FRAME");
check("round-trip 4 LED", decoded.leds.length === 4 && decoded.leds[0][0] === 255);
check("round-trip lunghezza", frameBytes.length === 4 + frameBytes[3]);

// 4. HELLO/RELEASE/LIST framing
check("HELLO framing", protocolDecode(protocolEncode({ cmd: "HELLO", ver: 1 })).cmd === "HELLO");
check("RELEASE framing", protocolDecode(protocolEncode({ cmd: "RELEASE", id: "abc" })).cmd === "RELEASE");
check("LIST framing", protocolDecode(protocolEncode({ cmd: "LIST" })).cmd === "LIST");

// 5. colori
check("hexToRgb #009bde", JSON.stringify(hexToRgb("#009bde")) === "[0,155,222]");
check("hexToRgb default nero", JSON.stringify(hexToRgb(undefined)) === "[0,0,0]");
check("normalizeColor array", JSON.stringify(normalizeColor([1, 2, 3])) === "[1,2,3]");
check("normalizeColor oggetto", JSON.stringify(normalizeColor({ r: 4, g: 5, b: 6 })) === "[4,5,6]");
check("normalizeColor buffer nativo", JSON.stringify(normalizeColor(Uint8Array.from([0,0,255]))) === "[0,0,255]");

// Initialize/Render/Shutdown reali: trasporto UDP, colore esatto e rilascio.
Object.assign(sandbox, {
  controller: {device:{id:'render-test',name:'Test',host:'127.0.0.1',port:21444}},
  bridgeHost:'127.0.0.1',bridgePort:21444,frameDelay:'33',lightingMode:'Colore fisso',
  forcedColor:'#0000ff',onShutdown:'Rilascia controllo',shutdownColor:'#000000',
  device:{log(){},setName(){},setImageFromBase64(){},setSize(){},setControllableLeds(){},
    color(){return [255,0,0];},createColorArray(c){return hexToRgb(c);}},
  Date:{now(){return 100000;}}
});
let schedulerTarget = 0;
sandbox.device.setFrameRateTarget = target => { schedulerTarget = target; };
sandbox.Initialize();
check("Initialize limita il runtime SignalRGB a 30 FPS", schedulerTarget === 30);
sandbox.Render();
const renderSocket = sockets.at(-1);
const rendered = renderSocket.packets.map(protocolDecode).find(p=>p.cmd==='FRAME');
check("Render invia blu esatto a quattro zone via UDP", rendered?.leds.every(c=>JSON.stringify(c)==='[0,0,255]'));
const packetCount = renderSocket.packets.length;
sandbox.Render();
check("Render limita la frequenza senza attendere ACK", renderSocket.packets.length===packetCount);
sandbox.Date.now = () => 100020;
sandbox.Render();
check("Render non supera il limite con canvas a 50 FPS", renderSocket.packets.length===packetCount);
sandbox.Date.now = () => 100033;
sandbox.Render();
check("Render non reinvia un colore identico a 33 ms", renderSocket.packets.length===packetCount);
sandbox.forcedColor = '#ff0000';
sandbox.Date.now = () => 100066;
sandbox.Render();
check("Render invia subito un colore diverso senza attenuazione", renderSocket.packets.length===packetCount+1 &&
  protocolDecode(renderSocket.packets.at(-1)).leds.every(c=>JSON.stringify(c)==='[255,0,0]'));
sandbox.Date.now = () => 100533;
sandbox.Render();
check("Render non duplica prima del keepalive", renderSocket.packets.length===packetCount+1);
sandbox.Date.now = () => 100566;
sandbox.Render();
check("Render mantiene attivo lo stream statico ogni mezzo secondo", renderSocket.packets.length===packetCount+2);
let nativeNotifications = 0;
sandbox.device.notify = () => { nativeNotifications++; };
renderSocket.handlers.error('offline');
check("errore socket non apre notifiche dal thread nativo", nativeNotifications === 0);
sandbox.Date.now = () => 200000;
sandbox.Render();
check("riconnessione senza accumulare socket nativi", sockets.at(-1) === renderSocket);

let renderTime = 200000;
sandbox.Date.now = () => renderTime;
sandbox.lightingMode = 'Canvas';
const transitionStart = renderSocket.packets.length;
for (const color of [[0,0,0], [255,255,255], [0,0,0]]) {
  sandbox.device.color = () => color;
  renderTime += 40;
  sandbox.Render();
}
const transition = renderSocket.packets.slice(transitionStart).map(protocolDecode);
check("Render conserva nero-bianco-nero senza attenuazione o colori residui",
  JSON.stringify(transition.map(p=>p.leds)) === JSON.stringify([
    Array(4).fill([0,0,0]), Array(4).fill([255,255,255]), Array(4).fill([0,0,0])
  ]));

const framesBeforeFailure = vm.runInContext('framesSent', sandbox);
const originalSend = renderSocket.send;
let failedSendAttempts = 0;
renderSocket.send = () => { failedSendAttempts++; return -1; };
sandbox.device.color = () => [0,255,0];
renderTime += 40;
sandbox.Render();
check("send -1 non viene contato come frame inviato", vm.runInContext('framesSent', sandbox) === framesBeforeFailure);
renderTime += 40;
sandbox.Render();
check("send -1 applica backoff senza accodare altri frame", failedSendAttempts === 1);
renderSocket.send = originalSend;
renderTime += 2000;
sandbox.Render();
renderTime += 40;
sandbox.Render();
check("ripresa dopo send -1 riusa il socket e invia il frame corrente",
  sockets.at(-1) === renderSocket && vm.runInContext('framesSent', sandbox) === framesBeforeFailure + 1);
sandbox.bridgeFrameRate = '5';
const ratePackets = renderSocket.packets.length;
sandbox.device.color = () => [12,34,56];
renderTime += 199;
sandbox.Render();
check("limite 5 FPS non invia prima di 200 ms", renderSocket.packets.length === ratePackets);
renderTime += 1;
sandbox.Render();
check("limite 5 FPS invia il colore corrente a 200 ms", renderSocket.packets.length === ratePackets + 1);
sandbox.Shutdown(false);
check("Shutdown rilascia controllo per spegnimento", protocolDecode(renderSocket.packets.at(-1)).cmd==='RELEASE');

// 6. errori di framing
let threw = false;
try { protocolDecode([0, 0, 0, 99, 1, 2]); } catch (e) { threw = true; }
check("frame incompleto rifiutato", threw);

console.log(`\n${pass} passati, ${fail} falliti`);
process.exit(fail ? 1 : 0);
