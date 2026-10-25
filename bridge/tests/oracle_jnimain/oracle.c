/* Oracle nativo (32-bit) per jnimain.dll — bundle LSC Battletron 2.0.17.
 * Stampa i casi sign/secret in formato TSV: TYPE\tINPUT\tOUTPUT_HEX
 * e la riga SIGCACHE\t<stringa catturata dal log printf della DLL.
 * Build: vcvars32.bat && cl /Fe:oracle.exe oracle.c user32.lib
 */
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef long (*fn_setSecretContent)(const char *, int);
typedef void (*fn_doCommand)(int, const char *, int, const char *, unsigned char, unsigned char, char *);
typedef void (*fn_getEncryptoKey)(const char *, int, const char *, char *);

static const char *APPKEY = "q594qaqdpy89gmvyndtp";
static const char *APPSECRET = "xh59e8qykn4sp7jyh7rwaq3ykfwf8e5n";
static const char *BMP = "t_s_online.bmp";

static char *read_file(const char *path, long *len) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    *len = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *buf = (char *)malloc(*len);
    if (fread(buf, 1, *len, f) != (size_t)*len) { fclose(f); free(buf); return NULL; }
    fclose(f);
    return buf;
}

static void hexout(const char *s, int n) {
    for (int i = 0; i < n; i++) printf("%02x", (unsigned char)s[i]);
}

int main(void) {
    HMODULE h = LoadLibraryA("jnimain.dll");
    if (!h) { printf("ERR load\n"); return 1; }
    fn_setSecretContent setSecretContent = (fn_setSecretContent)GetProcAddress(h, "setSecretContent");
    fn_doCommand doCommand = (fn_doCommand)GetProcAddress(h, "doCommand");
    fn_getEncryptoKey getEncryptoKey = (fn_getEncryptoKey)GetProcAddress(h, "getEncryptoKey");
    if (!setSecretContent || !doCommand || !getEncryptoKey) { printf("ERR exports\n"); return 1; }

    long blen = 0;
    char *bmp = read_file(BMP, &blen);
    if (!bmp) { printf("ERR bmp\n"); return 1; }
    if (setSecretContent(bmp, (int)blen) != 0) { printf("ERR setSecretContent\n"); return 1; }
    free(bmp);

    char out[64];
    /* comando 0: inizializza e logga "sig cache : <str>" su stdout */
    doCommand(0, APPSECRET, (int)strlen(APPSECRET), APPKEY, 0, 0, out);

    const char *cases[] = {
        "",
        "a",
        "a=1||b=2",
        "appRnVersion=5.27||appVersion=1.0.0||channel=oem||clientId=q594qaqdpy89gmvyndtp",
        "postData=0123456789abcdef0123456789abcdef||t=1700000000",
        "a=1||b=2||c=3||d=4||e=5||f=6||g=7",
        "tuya.m.user.qr.token.create",
        "requestId=6f6a2f9b8d7c4e5f9a1b2c3d4e5f6789||sid=abc123",
    };
    for (int i = 0; i < (int)(sizeof(cases) / sizeof(cases[0])); i++) {
        memset(out, 0, sizeof(out));
        doCommand(1, cases[i], (int)strlen(cases[i]), NULL, 0, 0, out);
        printf("SIGN\t%s\t", cases[i]);
        hexout(out, 32);
        printf("\n");
    }

    const char *reqs[] = {
        "00000000-0000-4000-8000-000000000000",
        "12345678-1234-4123-8123-123456789abc",
        "abcdef01-2345-6789-abcd-ef0123456789",
    };
    for (int i = 0; i < (int)(sizeof(reqs) / sizeof(reqs[0])); i++) {
        char o64[64];
        memset(o64, 0, sizeof(o64));
        getEncryptoKey(reqs[i], (int)strlen(reqs[i]), NULL, o64);
        printf("SECRET\t%s\t", reqs[i]);
        hexout(o64, 64);
        printf("\n");
        memset(o64, 0, sizeof(o64));
        getEncryptoKey(reqs[i], (int)strlen(reqs[i]), "ecodekeyvalue", o64);
        printf("SECRET_ECODE\t%s\t", reqs[i]);
        hexout(o64, 64);
        printf("\n");
    }
    return 0;
}
