// E2E dell'eseguibile release, con configurazione isolata e socket TCP reale.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import net from 'node:net';
import { once } from 'node:events';
import { protocolEncode, createProtocolReader } from '../plugin/lib/framing.js';

const root = mkdtempSync(join(tmpdir(), 'lscbridge-test-'));
const dir = join(root, 'LscBridge');
mkdirSync(dir);
const configPath = join(dir, 'config.json');
const exe = resolve(process.env.LSC_TEST_EXE || 'bridge/target/release/lscbridge.exe');
const env = { ...process.env, APPDATA: root };
let bridge, socket;
try {
  writeFileSync(configPath, '{broken');
  const bad = spawn(exe, ['status'], { env, windowsHide: true, stdio: 'ignore' });
  assert.equal((await once(bad, 'exit'))[0], 1);
  assert.equal(readFileSync(configPath, 'utf8'), '{broken');

  // A zero-filled primary must recover only from a valid backup, preserving evidence.
  writeFileSync(join(dir, 'config.json.backup'), JSON.stringify({devices: [{dev_id: 'recovered', name: 'Recovered', rgbic: true, local_key: null}]}));
  writeFileSync(configPath, Buffer.alloc(512));
  const recovery = spawn(exe, ['status'], {env, windowsHide: true, stdio: 'ignore'});
  assert.equal((await once(recovery, 'exit'))[0], 0);
  assert.equal(readFileSync(join(dir, 'config.json.corrupt')).length, 512);

  const reservation = net.createServer();
  reservation.listen(0, '127.0.0.1');
  await once(reservation, 'listening');
  const port = reservation.address().port;
  await new Promise(r => reservation.close(r));
  writeFileSync(configPath, JSON.stringify({bridge_port: port, devices: [
    {dev_id: 'smoke-device', name: 'Caffè 灯', rgbic: true, local_key: null}
  ]}));
  bridge = spawn(exe, ['run'], { env, windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] });
  await new Promise((resolveReady, reject) => {
    const timer = setTimeout(() => reject(new Error('timeout avvio bridge')), 5000);
    bridge.once('error', reject);
    bridge.once('exit', code => { clearTimeout(timer); reject(new Error('bridge terminato: ' + code)); });
    bridge.stderr.on('data', data => {
      if (data.toString().includes('control server su')) { clearTimeout(timer); resolveReady(); }
    });
  });
  socket = net.createConnection({host: '127.0.0.1', port});
  await once(socket, 'connect');
  const read = createProtocolReader();
  const queue = [];
  socket.on('data', data => queue.push(...read(data)));
  async function request(body) {
    const bytes = Buffer.from(protocolEncode(body));
    socket.write(bytes.subarray(0, 2));
    socket.write(bytes.subarray(2));
    const start = Date.now();
    while (!queue.length) {
      assert.ok(Date.now() - start < 3000, 'timeout risposta TCP');
      await new Promise(r => setTimeout(r, 10));
    }
    return queue.shift();
  }
  assert.equal((await request({cmd: 'HELLO'})).app, 'lscbridge');
  assert.equal((await request({cmd: 'LIST'})).devices[0].name, 'Caffè 灯');
  assert.equal((await request({cmd: 'FRAME', id: 'smoke-device', leds: Array(4).fill([255,0,0])})).ok, true);
  assert.ok((await request({cmd: 'FRAME', id: 'unknown', leds: Array(4).fill([0,0,0])})).error);
  assert.equal((await request({cmd: 'RELEASE', id: 'smoke-device'})).ok, true);
  assert.equal((await request({cmd: 'RECONNECT', id: 'smoke-device'})).ok, true);
  assert.ok((await request({cmd: 'RECONNECT', id: 'unknown'})).error);
  console.log('E2E release: config corrotta preservata; HELLO/LIST UTF-8/FRAME/errore/RELEASE OK');
} finally {
  socket?.destroy();
  if (bridge && bridge.exitCode === null) {
    const exited = once(bridge, 'exit');
    bridge.kill();
    await exited;
  }
  rmSync(root, {recursive: true, force: true});
}
