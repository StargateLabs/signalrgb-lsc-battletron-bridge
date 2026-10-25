// SignalRGB riceve un solo file: i moduli di supporto non vengono indicizzati
// dal crawler come plugin separati. Nessuna dipendenza di bundling aggiuntiva.
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {dirname, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = readFileSync(resolve(root,'plugin/BattletronBridge.js'),'utf8');
const helpers = readFileSync(resolve(root,'plugin/lib/framing.js'),'utf8').replace(/^export /gm,'');
const imports = /^import .* from "\.\/lib\/framing\.js";\r?$/gm;
if ([...source.matchAll(imports)].length !== 1) throw new Error('Import framing non riconosciuto');
const bundle = source.replace(imports, () => helpers);
if (/from ["']\.\//.test(bundle)) throw new Error('Il bundle contiene ancora import relativi');
const output = resolve(root,'plugin/dist/BattletronBridge.js');
mkdirSync(dirname(output),{recursive:true});
writeFileSync(output,bundle,'utf8');
const check = spawnSync(process.execPath,['--check',output],{encoding:'utf8'});
if (check.status !== 0) throw new Error(check.stderr || 'Bundle non valido');
console.log('Plugin standalone generato: ' + output);
