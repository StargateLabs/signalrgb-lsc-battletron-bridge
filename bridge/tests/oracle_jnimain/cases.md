# Casi oracolo â€” jnimain.dll (bundle LSC Battletron 2.0.17)

Fonte: `jnimain.dll` (x86, SHA-256 vedi ledger) eseguita via l'oracolo nativo
`oracle.c` (MSVC Build Tools x86), con `t_s_online.bmp` e il clientId/appSecret OEM
dal bundle. I binari vendor **non** sono versionati: ricostruirli estraendo
`resources/static/dll/` dal 7z-SFX originale con 7-Zip.

## sigStrCache (derivato da BMP + appSecret, loggato dalla DLL)

```
A_yegjfwuukevd8qfxw3rjfr5sj43p5gpr_xh59e8qykn4sp7jyh7rwaq3ykfwf8e5n
```

## sign(command 1) = hex(HMAC-SHA256(key=sigStrCache, msg=str)) â€” 64 char

| input | firma |
|---|---|
| `` (vuoto) | b42a11c88c37af33b5cb6752a54d92ec5b7d034d3f499bd5c5fea39414eb4963 |
| `a` | b17a597fde294a1818dc44574c9baf1a544bd95b3e64ca07206acfd5e4239950 |
| `a=1||b=2` | 38cf9902909887f50ea01987867eb69c4bde59f8da346185047962ab3fa777e8 |
| `appRnVersion=5.27||appVersion=1.0.0||channel=oem||clientId=q594qaqdpy89gmvyndtp` | cb24c2f4ead05558c75169b69c5ea917fb6017a6dff7c14629d01cf5a3dc95ca |
| `postData=0123456789abcdef0123456789abcdef||t=1700000000` | 2e2f18d97c6de03936a79c9dc1ef81c03e00d023265788c897c0d3637d7b82a1 |
| `a=1||b=2||c=3||d=4||e=5||f=6||g=7` | fbb2718d47fe872b12f2caa9179bdf99d897fa61a28a7d2fd2dc83f7e33eb384 |
| `tuya.m.user.qr.token.create` | fe4c36250c5d5b6e7e908127fb45107c28e6bb81bf46f2c0e389224f6849fa94 |
| `requestId=6f6a2f9b8d7c4e5f9a1b2c3d4e5f6789||sid=abc123` | f5eb181d7bcbcc1164a1deecf36fe5aa437fca4864eb704113d1b5dec15bc173 |

## secret(requestId, ecodeKey?) = HMAC-SHA256(key=requestId, msg=sigStrCache[+"_"+ecode]) â€” primi 16 hex

| requestId | ecode | secret |
|---|---|---|
| 00000000-0000-4000-8000-000000000000 | â€” | 7ca1c563b051af7a |
| 00000000-0000-4000-8000-000000000000 | ecodekeyvalue | 749418ad952880a8 |
| 12345678-1234-4123-8123-123456789abc | â€” | f18580c627b4a2f2 |
| 12345678-1234-4123-8123-123456789abc | ecodekeyvalue | b68d9d3e99315714 |
| abcdef01-2345-6789-abcd-ef0123456789 | â€” | 415275f8213c175c |
| abcdef01-2345-6789-abcd-ef0123456789 | ecodekeyvalue | 9876edd257c5a57e |

## Rigenerazione

```powershell
call "C:\BuildTools\VC\Auxiliary\Build\vcvars32.bat"
cd bridge\tests\oracle_jnimain
cl /nologo /Fe:oracle.exe oracle.c user32.lib
# copiare jnimain.dll e t_s_online.bmp dal bundle in questa cartella
.\oracle.exe    # stampa i casi SIGN/SECRET e il sig cache dal log della DLL
```

I test in `bridge/src/cloud/sign.rs` fissano tutti questi valori.

