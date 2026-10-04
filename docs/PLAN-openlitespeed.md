# PLAN — OpenLiteSpeed as a per-site override server: the macOS self-build, proven

**Status:** IN PROGRESS — prototype proven 4 Oct 2026 on the dev Mac (arm64, macOS 26.6, Apple
clang 21). **Owner ruled §5 on 4 Oct 2026: go.** Windows refused (1), patch both (2, 3), Linux
from the same recipe (4). P1 shipped (runtimes PR #15, release `openlitespeed-1.9.3-1`);
**P2 and P3 shipped 4 Oct 2026** (§6) — `openlitespeed_site_check` green on macOS and the
Ubuntu VM; every running-app SMOKE row 28/28 on the macOS 15.8 VM and the Ubuntu 22.04 VM,
LSCache purge-on-edit included. Open: the Windows refusal rows (no reachable Windows host). Open work is
the "OpenLiteSpeed override server" row in `docs/TODO.md`; this file is the evidence and the recipe.

### Rulings (owner, 4 Oct 2026)

1. **Windows: refused**, in CORE through the pins, with a `PlatformWords` sentence naming the
   upstream fact. The first feature whose third OS is blocked by the upstream itself.
2. **`/tmp` paths: patched** — `0003-runtime-tmp-dir`: `LSWS_TMP_DIR` replaces every compiled-in
   `/tmp/lshttpd` use (pid, graceful pid, swap, status links, cgid and per-vhost sockets, test
   log, core backups) and the `/tmp/ols/shm` fallback. `DEFAULT_TMP_DIR` became a function call so
   the COMPILER proves every concatenating site was converted. The configured shm dir needed no
   patch: `shmDefaultDir` belongs inside `tuning {}` (the prototype put it at server level,
   which OLS ignores — §4 corrected).
3. **Phone-home: patched** — `0004-no-remote-fetch`: `noRemoteFetch 1`. Two fetches, not one:
   besides quic.cloud, `checkOLSUpdate()` GETs `openlitespeed.org/packages/release?ver=…&os=…`
   on the first timer tick and every 30 minutes, reporting version, OS and platform.
4. **Linux: same recipe** on `ubuntu-22.04` / `-arm` runners, glibc 2.35, libstdc++ static.
5. **Scope: go.**

The TODO row said OLS was blocked because no macOS artifact exists, the one community tap was
frozen at 1.4.51, and a self-build needed hosting infra rexenv did not have. **All three were
stale by Oct 2026**: `puleeno/homebrew-openlitespeed` landed 1.9.1 with a macOS patch set and an
`arm64_sonoma` bottle on 4 Aug 2026 (built on a `macos-14` runner), and `rexenv/runtimes` has
built PHP 7.4/8.x and Nginx for all three OSes since Sep 2026. What was still unproven — whether
a current OLS boots on macOS with the **cache module on** (the tap disables it: "segfaults at
startup") — is what this prototype settles.

---

## 0. Verdict

- **macOS: buildable and working.** OLS 1.9.3 (released 30 Sep 2026) builds from source with a
  ~700-line Darwin patch (the tap's, rebased: three hunks dropped because upstream 1.9.3 already
  carries them) plus four fixes found here (§3). The binary is 8.4 MB, arm64, and its dylib
  closure is `/usr/lib` only (libSystem, libc++, libz, libexpat, libxml2) — no Homebrew dylib.
- **It fits rexenv's override shape exactly.** Loopback listener, `disableWebAdmin 1` (no admin
  console, no `admin_php`, no lsphp), PHP through the EXISTING per-version php-fpm pool over
  FastCGI (`extProcessor type fcgi … autoStart 0`, same class as Apache's `mod_proxy_fcgi`),
  `.htaccess` rewrites, and the **cache module ON with `x-litespeed-cache: miss` then `hit`** —
  the thing the LiteSpeed Cache plugin needs and the tap could not deliver.
- **Linux: cheap.** Official `openlitespeed-1.9.3-{x86_64,aarch64}-linux.tgz` on the GitHub
  release (95 MB; `bin/openlitespeed` needs glibc ≥ 2.11, so the 22.04 floor holds).
- **Windows: no path, ever.** Upstream's `build.sh` quits on anything but Linux, macOS and
  FreeBSD; the server is a fork-based POSIX codebase and nobody has ported it. §5.1.

## 1. What was measured (4 Oct 2026, `proto/run.sh`, server on `127.0.0.1:8480`)

| Probe | Result |
|---|---|
| `openlitespeed -v` | `LiteSpeed/1.9.3 Open`, `lsquic 4.10.0` |
| `GET /index.html` | 200, `server: LiteSpeed` |
| `GET /index.php` | 200 via rexenv's running php-fpm 8.3 pool (`127.0.0.1:9783`): `php=8.3.32 sapi=fpm-fcgi server=LiteSpeed`, `SCRIPT_FILENAME` = the docroot file |
| `GET /index.php?k=<fresh>` ×2 | `x-litespeed-cache: miss`, then `hit`; entries under `cachedata/`; static file carries no cache header |
| `GET /rewritten/abc` | `.htaccess` `RewriteRule` → `index.php`, `REQUEST_URI` preserved |
| `GET /sub/` | subdirectory index served |
| process after probes | alive; stops clean on SIGTERM |
| config test `-t` | exit 0 once `statDir` is set (§3.1) |

Two warnings are cosmetic and one is not: "Gid of docroot is 20, smaller than minimum 100, use
server gid" (a `CGIRLimit`-class check; `staff` is gid 20 on macOS); "path is not accessible:
`$SERVER_ROOT/share/autoindex/`" (create the dir or ship no autoindex); and **"HttpFetch: SSL
Verify failed"** — OLS tried to download `https://quic.cloud/ips` at boot (§5.3).

## 2. The build recipe

> **Superseded for the artifact by `rexenv/runtimes` `scripts/build-openlitespeed.sh` (PR #15,
> 4 Oct 2026)** — the same recipe for all four targets, every dependency static from pinned source
> (pcre2/zlib/expat too, no Homebrew), the six-patch set, and the gates of §3.1. The script below
> is kept as the prototype's record; read the runtimes script for anything you will build.

Reproducible from pristine sources; the seed of a `rexenv/runtimes` workflow. Build system is
**CMake, upstream's own path** (`build.sh` → `updateSrcCMakelistfile()` + its Darwin seds), not
the tap's `./configure` — the autotools `Makefile.am`s are stale (no scgi/uwsgi/ssl4conn) and the
tap had to patch them; CMake needs nothing. Deps are laid out the way upstream's `third-party/`
scripts do on Linux (`../third-party/{lib,include}` beside the source, BoringSSL in `ssl/`).

```bash
#!/bin/bash
# OpenLiteSpeed 1.9.3 on macOS — proven 4 Oct 2026 (arm64). Run with a work dir: ./build-ols-macos.sh ~/ols-build
set -euo pipefail
W="${1:-$PWD/ols-build}"; mkdir -p "$W"; cd "$W"; JOBS="${JOBS:-8}"
OLS_VER=1.9.3
LSQUIC_COMMIT=d5929af7cec6fd74f1cfea2cb1c07c27ce9102b1   # olssrc/LSQUICCOMMIT
BSSL_COMMIT=9fc1c33e9c21439ce5f87855a6591a9324e569fd     # the tap's pin (2023-06-08); upstream dlbssl.sh pins a 2019 commit
BROTLI_VER=1.1.0
BCRYPT_COMMIT=55ff64349dec3012cfbbb1c4f92d4dbd46920213
TAP_RAW=https://raw.githubusercontent.com/puleeno/homebrew-openlitespeed/HEAD
fetch() { curl -sfL --retry 5 --retry-delay 5 "$1" -o "$2"; }

# 1. sources — src/liblsquic, src/lshpack and include/lsquic*.h are symlinks into ./lsquic in the tarball
[ -d olssrc ] || { fetch https://github.com/litespeedtech/openlitespeed/archive/refs/tags/v$OLS_VER.tar.gz ols-src.tgz
  mkdir olssrc && tar xzf ols-src.tgz -C olssrc --strip-components=1; }
cd olssrc
if [ ! -f lsquic/include/lsquic.h ]; then
  rmdir lsquic 2>/dev/null || true
  git clone --quiet --filter=blob:none https://github.com/litespeedtech/lsquic.git lsquic
  (cd lsquic && git checkout --quiet "$LSQUIC_COMMIT" && git submodule update --init --quiet src/liblsquic/ls-qpack src/lshpack)
fi

# 2. the tap's macOS patch, rebased: 3 hunks reject on 1.9.3 (useacme VLA, lscgid stubs, extensions Makefile.am) — upstream has them
if [ ! -f .macos-patched ]; then
  fetch $TAP_RAW/patches/ols191-macos.patch ../ols191-macos.patch
  patch -p1 -N -i ../ols191-macos.patch >/dev/null || true
  [ -f src/extensions/cgi/lscgid.cpp.orig ] && mv src/extensions/cgi/lscgid.cpp.orig src/extensions/cgi/lscgid.cpp  # keep upstream's
  find . -name '*.rej' -delete; find . -name '*.orig' -delete; touch .macos-patched
fi

# 3. deps → ../third-party and ./ssl
TP="$W/third-party"; mkdir -p "$TP/lib" "$TP/include"; cd "$W"
if [ ! -f olssrc/ssl/libdecrepit.a ]; then
  [ -d boringssl ] || { fetch https://github.com/google/boringssl/archive/$BSSL_COMMIT.tar.gz bssl.tgz
    mkdir boringssl && tar xzf bssl.tgz -C boringssl --strip-components=1; }
  fetch $TAP_RAW/patches/bssl-lstls.patch bssl-lstls.patch
  # This BoringSSL GENERATES crypto/err_data.c with Go and cmake/go.cmake is FATAL without one. CI runners have Go;
  # without it, stub Go and run err_data_generate.py (§2.1) — an EMPTY err_data.c still links libcrypto.a and only
  # the final openlitespeed link reports kOpenSSLReasonValues missing, hence the nm check.
  GO_FLAG=""; command -v go >/dev/null || GO_FLAG="-DGO_EXECUTABLE=/usr/bin/true"
  (cd boringssl && patch -p1 -N -i ../bssl-lstls.patch >/dev/null || true
   mkdir -p build && cd build && cmake .. -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_FLAGS="-fPIC" -DCMAKE_CXX_FLAGS="-fPIC" -DBUILD_TESTING=OFF $GO_FLAG >/dev/null
   if [ -n "$GO_FLAG" ]; then (cd ../crypto/err && python3 "$W/err_data_generate.py" > ../../build/crypto/err_data.c); fi
   make -j"$JOBS" crypto ssl decrepit >/dev/null
   nm crypto/libcrypto.a | grep -q ' S _kOpenSSLReasonValues$' || { echo "libcrypto.a lacks kOpenSSLReasonValues"; exit 1; })
  mkdir -p olssrc/ssl && cp boringssl/build/crypto/libcrypto.a boringssl/build/ssl/libssl.a boringssl/build/decrepit/libdecrepit.a olssrc/ssl/
  cp -r boringssl/include olssrc/ssl/
  ln -sfn ssl olssrc/openssl        # CMakeModules/common.cmake includes ${PROJECT_SOURCE_DIR}/openssl/include
fi
if [ ! -f "$TP/lib/libbrotlicommon-static.a" ]; then
  fetch https://github.com/google/brotli/archive/refs/tags/v$BROTLI_VER.tar.gz brotli.tgz
  mkdir -p brotli && tar xzf brotli.tgz -C brotli --strip-components=1
  (cd brotli && mkdir -p out && cd out && cmake .. -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DBROTLI_DISABLE_TESTS=ON >/dev/null && make -j"$JOBS" >/dev/null)
  for a in libbrotlidec libbrotlienc libbrotlicommon; do cp brotli/out/$a.a "$TP/lib/$a-static.a"; done
  cp -r brotli/c/include/brotli "$TP/include/"
fi
if [ ! -f "$TP/lib/libbcrypt.a" ]; then
  fetch https://github.com/litespeedtech/libbcrypt/archive/$BCRYPT_COMMIT.tar.gz libbcrypt.tgz
  mkdir -p libbcrypt && tar xzf libbcrypt.tgz -C libbcrypt --strip-components=1
  (cd libbcrypt && make -s >/dev/null 2>&1); cp libbcrypt/bcrypt.a "$TP/lib/libbcrypt.a"; cp libbcrypt/bcrypt.h "$TP/include/"
fi
if [ ! -f "$TP/lib/libudns.a" ]; then
  # udns 0.4's configure probes inet_pton() with an implicit declaration, which clang 16+ rejects, so it concludes
  # "no", compiles its own udns_pton, and dns_pton is UNDEFINED at the final link. The define tells it the truth.
  fetch https://deb.debian.org/debian/pool/main/u/udns/udns_0.4.orig.tar.gz udns.tgz
  mkdir -p udns && tar xzf udns.tgz -C udns --strip-components=1
  (cd udns && CFLAGS="-O2 -Wno-implicit-function-declaration -DHAVE_INET_PTON_NTOP" ./configure --disable-ipv6 >/dev/null && make -s libudns.a >/dev/null 2>&1 || true)
  nm udns/libudns.a | grep -q ' T _dns_pton$' || { echo "libudns.a lacks dns_pton"; exit 1; }
  cp udns/libudns.a "$TP/lib/"; cp udns/udns.h "$TP/include/"
fi
ln -sf "$(brew --prefix pcre2)/lib/libpcre2-8.a" "$TP/lib/libpcre2-8.a"

# 4. upstream build.sh's updateSrcCMakelistfile() + its Darwin seds, plus three of ours
cd olssrc
if [ ! -f .cmake-edited ]; then
  co(){ sed -i '' -e "s|$1|#$1|g" "$2"; }
  for l in 'add_definitions(-DRUN_TEST)' 'add_definitions(-DPOOL_TESTING)' 'add_definitions(-DTEST_OUTPUT_PLAIN_CONF)' \
           'add_definitions(-DDEBUG_POOL)' 'set(libUnitTest' 'find_package(ZLIB' 'find_package(PCRE' 'add_subdirectory(test)' \
           'SET (CMAKE_C_COMPILER' 'SET (CMAKE_CXX_COMPILER' \
           'set(IP2LOC_ADD_LIB' 'add_definitions(-DUSE_IP2LOCATION)' 'set(MMDB_LIB' 'add_definitions(-DENABLE_IPTOGEO2)'; do co "$l" CMakeLists.txt; done
  sed -i '' -e 's/\${unittest_STAT_SRCS}//g; s/libstdc++\.a//g; s/-nodefaultlibs //g; s/ rt//g; s/ crypt//g; s/gcc_eh//g; s/c_nonshared//g; s/gcc//g; s/-Wl,--whole-archive//g; s/-Wl,--no-whole-archive//g; s/libz\.a/z/g; s/libxml2\.a/xml2/g; s/libexpat\.a/expat/g' src/CMakeLists.txt
  co ls_llmq.c src/lsr/CMakeLists.txt; co ls_llxq.c src/lsr/CMakeLists.txt
  co 'add_subdirectory(modacme)' src/modules/CMakeLists.txt   # links -nodefaultlibs libstdc++.a; no libstdc++ on macOS; ACME unused under our CA
  sed -i '' -e 's|link_directories("/usr/lib64")|link_directories("/usr/lib64" "${PROJECT_SOURCE_DIR}/ssl" "${PROJECT_SOURCE_DIR}/../third-party/lib")|' src/CMakeLists.txt
  touch .cmake-edited
fi

# 5. build. The include flag names pcre2's OWN dir — never -I/opt/homebrew/include: with ssl/ absent at compile time that
# resolved <openssl/ssl.h> to Homebrew's OpenSSL 3, whose header renames SSL_get_peer_certificate to SSL_get1_peer_certificate,
# a symbol BoringSSL does not have. Lay ssl/ out BEFORE the first compile (this script does) and keep the flag narrow.
mkdir -p build && cd build
cmake -DCMAKE_BUILD_TYPE=Release -DMOD_PAGESPEED=OFF -DMOD_SECURITY=OFF -DMOD_LUA=OFF \
      -DCMAKE_C_FLAGS="-I$(brew --prefix pcre2)/include" -DCMAKE_CXX_FLAGS="-I$(brew --prefix pcre2)/include" .. >/dev/null
make -j"$JOBS"
ls -la src/openlitespeed && src/openlitespeed -v
```

### 2.1 `err_data_generate.py` — the Go generator, ported

A line-for-line port of BoringSSL `crypto/err/err_data_generate.go` at the pinned commit. Proven
byte-identical: the real `go run` output and this script's output `cmp` equal (4 Oct 2026). Only
needed on a host without Go; GitHub runners have Go and the recipe then uses it.

```python
#!/usr/bin/env python3
"""Port of BoringSSL crypto/err/err_data_generate.go (commit 9fc1c33) — run inside crypto/err/."""
import os, sys
LIBS = ["NONE","SYS","BN","RSA","DH","EVP","BUF","OBJ","PEM","DSA","X509","ASN1","CONF","CRYPTO","EC","SSL","BIO",
        "PKCS7","PKCS8","X509V3","RAND","ENGINE","OCSP","UI","COMP","ECDSA","ECDH","HMAC","DIGEST","CIPHER","HKDF",
        "TRUST_TOKEN","USER"]
libmap = {n: i + 1 for i, n in enumerate(LIBS)}
OFFSET_MASK = 0x7fff
entries, interned, data = [], {}, bytearray()
def add(key, value):
    assert key & OFFSET_MASK == 0
    off = interned.get(value)
    if off is None:
        off = len(data); assert off & OFFSET_MASK == off, "stringList overflow"
        data.extend(value.encode()); data.append(0); interned[value] = off
    for e in entries:
        if e >> 15 == key >> 15: raise SystemExit("duplicate entry")
    entries.append(key | off)
for name in sorted(os.listdir(".")):
    if not name.endswith(".errordata"): continue
    with open(name, "rb") as f:
        for ln, line in enumerate(f.read().split(b"\n"), 1):
            if not line: continue
            parts = line.split(b",")
            assert len(parts) == 3, (name, ln)
            lib = libmap[parts[0].decode()]; assert lib < 64
            key = int(parts[1]); assert key < 2048
            add(lib << 26 | key << 15, parts[2].decode())
entries.sort(key=lambda e: e >> 15)
out = sys.stdout
out.write("""/* Copyright (c) 2015, Google Inc.
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
 * SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
 * OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
 * CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE. */

 /* This file was generated by err_data_generate.go. */

#include <openssl/base.h>
#include <openssl/err.h>

#include <assert.h>

""")
for i, n in enumerate(LIBS):
    out.write('static_assert(ERR_LIB_%s == %d, "library value changed");\n' % (n, i + 1))
out.write('static_assert(ERR_NUM_LIBS == %d, "number of libraries changed");\n\n' % (len(LIBS) + 1))
out.write("const uint32_t kOpenSSLReasonValues[] = {\n")
for v in entries: out.write("    0x%x,\n" % v)
out.write("};\n\nconst size_t kOpenSSLReasonValuesLen = sizeof(kOpenSSLReasonValues) / sizeof(kOpenSSLReasonValues[0]);\n\n")
out.write('const char kOpenSSLReasonStringData[] =\n    "')
for c in data:
    out.write('\\0"\n    "' if c == 0 else chr(c))
out.write('";\n\n')
```

### 2.2 What the prototype did NOT do (the artifact still needs)

- **x86_64 slice** — not built; same recipe on an Intel runner (`macos-13`) or
  `CMAKE_OSX_ARCHITECTURES=x86_64` for every dep. The tap publishes arm64 only.
- **minos floor** — add `-mmacosx-version-min=12.0` to every dep and OLS, as Nginx's build does
  (`docs/PORTS.md`); the prototype was built for the host.
- **Static zlib/expat/libxml2** — optional: the closure is `/usr/lib` only, which every macOS has;
  `prepare_binary_tree` has nothing to relink. Decide in P1.
- **Licence tarball** — OLS is GPLv3 and rexenv becomes the DISTRIBUTOR; `is_self_distributed`
  makes the catalog refuse a resolve without `licenses-*.tar.gz` (ledger #336). The patch set and
  pinned sources published beside the artifact in `rexenv/runtimes` are the corresponding-source
  offer. `THIRD-PARTY-NOTICES.md` gains OLS + BoringSSL + lsquic + brotli + udns + libbcrypt rows.

## 3. The Darwin defects met, and what each costs rexenv

1. **`statDir` missing → `strcmp(NULL)` segfault at startup, before the first log line** —
   `httpserver.cpp` `configServerBasics`: with no `statDir` it probes `/dev/shm`, which does not
   exist on macOS, leaves the pointer NULL and compares it. Even `-t` dies with 139. **The generator
   MUST emit `statDir <app-data>/ols/<site>/status`** on macOS (and may on Linux). A two-line
   upstream fix (fall back to `DEFAULT_TMP_DIR`) belongs in the patch set; report upstream.
2. **udns 0.4 configure vs clang 16+** — implicit-declaration probe fails, `dns_pton` undefined at
   link. `-DHAVE_INET_PTON_NTOP` (recipe step 3).
3. **`mod_acme` links `libstdc++.a`** — no libstdc++ on macOS; dropped (we have our own CA).
4. **BoringSSL needs Go** to generate `err_data.c`; §2.1.
5. **Include-order trap** — `-I/opt/homebrew/include` for pcre2 pulled OpenSSL 3's headers while
   `ssl/` was absent, and the object compiled against the wrong `SSL_get_peer_certificate` macro.
   Recipe step 5 narrows the flag; a `windows-check`-style trap for the runtimes workflow: build
   deps before the first OLS compile, never add Homebrew's umbrella include dir.
6. **Not met, tap-reported:** "cache module segfaults at startup" and "QUIC shm init crashes".
   Neither happened here — the cache module ran with `storagepath` under the server root and the
   `shmDefaultDir`/`statDir` directives set; QUIC was left off (`quicEnable 0`) because the edge is
   Caddy and OLS sits on loopback HTTP. The tap's crash is most likely defect 1 under a different
   config, but that is an inference, not a measurement.

### 3.1 What P1 met (4 Oct 2026, building the artifact)

Each line is a defect the runtimes gate caught, and what it costs or teaches P2.

- **PlainConf silently drops an unregistered directive.** `noRemoteFetch 1` parsed, logged
  `Not support [noremotefetch 1]` as a WARN, and the fetches still fired. Patch 0004 registers
  the keyword. **P2:** any directive rexenv emits must appear in OLS's keyword table — a typo is a
  WARN, not an error, and `-t` still exits 1, not 2.
- **`-t` exit codes: 0 clean, 1 warnings, 2 errors** (`lshttpdmain.cpp`). macOS's `staff` group is
  gid 20, under OLS's 100 minimum, so every macOS docroot draws a WARN. **P2:** treat 2 as failure,
  1 as fine.
- **A group-change command at every start**: `dseditgroup -o edit -a lsadm …` (macOS) /
  `usermod -a -G … lsadm` (Linux), printing "Username and password must be provided." — patch 0005.
- **`share/autoindex/` must exist** under the server root even with `autoIndex 0`, or the vhost
  logs an ERROR and `-t` exits 2. The artifact ships it empty.
- **`fileAccessControl { requiredPermissionMask 000 restrictedPermissionMask 000 }`** — OLS's
  defaults refused a plain `0644` file in a `mktemp` docroot ("does not meet the requirements of
  'Required bits'"). **P2:** emit both, like the prototype did by accident.
- **The cache block must be upstream's whole default block.** A short one (no `maxCacheObjSize`
  & co.) parses and never answers `hit`.
- **The cache manager is not up until the first timer tick** (~1–2 s after start): requests before
  it are served and never cached (0/6 hits without a wait, 18/18 with one). Harmless for a dev
  site; a probe in P2 must wait.
- **An uninitialised pointer read at the end of every request, on macOS only** — patch 0006.
  `HttpSession`'s constructor sets `m_pAioReq(NULL)` only under the Linux/AIO `#if`, while
  `releaseResources()` tests it unconditionally. macOS 26's heap usually handed back zeroes (one
  crash in ~80 local starts); the `macos-15` CI runner crashed on its first request, and a
  macOS 15.8 VM 10 times in 20. An ASan build pinned it on any macOS (`x0 = 0xbebebebe…`,
  `LsAioReq::isPending ← cancelReleaseAio ← releaseResources`). After the patch: 0/28 on 15.8,
  80 requests clean under ASan. **Lesson:** a gate that passes on the dev Mac proved nothing about
  the oldest macOS the artifact claims (`minos 12.0`) — the runner on an older macOS is what
  found it. Crash report kept in the prototype dir.
- **Process title is rewritten** to `openlitespeed (lshttpd - main)`, so the cmdline no longer
  carries the binary path: a `pkill -f <path>` left a server running here. **P2:** ownership for
  `adopt_startup` cannot be the binary path in the cmdline; use the pid file under
  `LSWS_TMP_DIR` + our fixed port, the way the edge is adopted through its admin socket.
- **Linux:** BoringSSL's aarch64 assembly needs clang (GCC 11 rejects `__has_feature` in `.S`);
  `libatomic` and `libaio` linked dynamically upstream — both now static; `<sys/capability.h>`
  needs `libcap-dev` (header only). Result: `NEEDED` = glibc + `libcrypt.so.1`, highest symbol
  `GLIBC_2.34`.
- **Script hygiene, twice:** `nm … | grep -q` under `pipefail` reports a PASSING check as failed
  (grep's early exit SIGPIPEs the producer), and editing a running bash script corrupts the run
  (bash reads it lazily). The runtimes script captures before grepping; runs went from a snapshot.

## 4. What the config generator must emit (measured working set)

Server (`conf/httpd_config.conf`, every path absolute and quoted-safe — app-data paths have
spaces; OLS's plain-text format tolerates them unquoted in this prototype, verify with a spaced
path in P2): `serverName`, `user`/`group` (the app user; OLS is never root here),
`disableWebAdmin 1`, `noRemoteFetch 1` (patch 0004), `swappingDir`, `mime conf/mime.properties`
(ship upstream's `dist/conf/mime.properties`), `httpdWorkers 1`, `autoRestart 0` (the
ServiceManager watchdog owns restarts), `tuning { shmDefaultDir <app> quicEnable 0 quicShmDir <app> … }` (shmDefaultDir is read from
`tuning`, NOT server level), and the process env `LSWS_TMP_DIR=<app>/ols/<site>/run` (patch 0003;
`statDir` then defaults there via patch 0002),
`listener Default { address 127.0.0.1:<override_port> secure 0 map <site> * }`,
`extProcessor <pool> { type fcgi address 127.0.0.1:<fpm_port> autoStart 0 persistConn 1 … }`,
`scriptHandler { add fcgi:<pool> php }`, `virtualHost <site> { vhRoot <docroot> configFile … }`,
`module cache { ls_enabled 1 storagepath <app>/ols/<site>/cache enableCache 0 … }` (the LSCache
plugin turns caching on per request; `enableCache 0` keeps plain sites uncached).

Vhost (`conf/vhosts/<site>/vhconf.conf`): `docRoot $VH_ROOT/`, `rewrite { enable 1
autoLoadHtaccess 1 }`, `context / { location $DOC_ROOT/ rewrite { RewriteFile .htaccess } }`,
`index { indexFiles index.html, index.php autoIndex 0 }`, logs under the site's log dir. The
three multisite rewrite templates become OLS `rewrite { rules … }` blocks in the same
Apache-dialect syntax Apache's override uses — the one override backend where the templates can
be shared nearly verbatim.

Open in P2: **per-site env vars**. `extProcessor env` lines reach only apps OLS spawns
(`autoStart 0` → never). The Apache override rides `SetEnv` → FCGI params; the OLS equivalent is
to be measured (candidates: `RewriteRule … [E=VAR:value]` in the vhost rewrite block, or a
`context` `extraHeaders`/`addDefaultCharset`-class directive). Until measured, OLS sites refuse
`site_env` like FrankenPHP once did — honest, in CORE.

## 5. Rulings needed before P1

1. **Windows.** OLS has no Windows build and will not get one. Ship macOS + Linux and refuse on
   Windows **in CORE through the pins** — `ensure_server_available_on` already does that for
   Apache and FrankenPHP, neither of which has a Windows artifact — with a `PlatformWords` sentence
   that names the upstream fact ("OpenLiteSpeed has no Windows build"), a permanent-blocked TODO
   row for the Windows third, and a §7 trap line in `docs/PLATFORMS.md`. This is the first feature
   whose third OS is blocked by the upstream rather than by us; the owner decides whether the
   three-OS rule admits that class at all.
2. **Paths outside app-data.** OLS compiles `DEFAULT_TMP_DIR=/tmp/lshttpd` and
   `PID_FILE=/tmp/lshttpd/lshttpd.pid` in (`src/config.h.cmake`), writes `.status`/`.rtreport`
   symlinks there, and put its system shm (`SSL.shm`, `adns_cache.shm`) in `/tmp/ols/shm/` even
   with `shmDefaultDir` set (why the configured dir did not win is unmeasured — `getValidFile` on a
   directory is the suspect). Two rexenv sites or two users would share one pid file. Options:
   (a) patch OLS to read `LSWS_TMP_DIR` from the environment for both constants and prefer the
   configured shm dir (~15 lines, recommended — the same patch set already exists); (b) accept
   `/tmp` on single-user dev machines. Ownership marking for `adopt_startup` needs the cmdline
   anyway (OLS is launched as `<app-data>/bin/openlitespeed-<ver>/bin/openlitespeed`, root derived
   from the binary's parent — one server root per site is the natural layout).
3. **Boot-time fetch.** `maybeDownloadQuicCloudTrustIp` downloads `https://quic.cloud/ips` on
   start, cached 24 h in `$SERVER_ROOT/tmp/download-quic-cloud-ips`, no directive to turn it off
   (measured: "SSL Verify failed" in the log — BoringSSL has no CA bundle here). A local dev
   server phoning a CDN at every boot is a privacy and offline-first problem. Options: (a) patch —
   a `quicCloudTrustIp 0` directive (recommended); (b) pre-touch the cache file at every start.
4. **Linux artifact.** Repack the official 95 MB tarball (only `bin/openlitespeed` and
   `conf/mime.properties` are needed; dynamic `libcrypt.so.1`, and a `liblve.so.0` string that may
   be a dlopen) vs. run the same recipe on `ubuntu-22.04`/`-arm` runners for a static ELF and ONE
   patch set across OSes (recommended — the rulings in 2 and 3 are patches, which the official
   tarball cannot carry).
5. **Scope.** The point of OLS for a WordPress developer is LSCache parity with LiteSpeed hosts
   (Hostinger, A2, …) and `.htaccess`. Apache already gives `.htaccess`; the unique value is the
   cache module + ESI, which this prototype proves. If that is not worth P1–P3 (~3–4 days), the
   TODO row closes as "won't do" with this file as the record.

## 6. Work plan, if ruled go

- **P1 — `rexenv/runtimes` workflow `openlitespeed.yml`** (one release `openlitespeed-1.9.3-1`):
  macOS arm64 + x86_64 from §2 with the patch set as files in the repo, minos 12.0, checksums,
  `licenses-openlitespeed-<ver>-<os>-<arch>.tar.gz`; Linux x86_64 + aarch64 on 22.04 runners. Done
  when `scripts/check-app-manifest` sees the pins and a clean VM resolves them.
  **State, 4 Oct 2026:** rexenv/runtimes PR #15 (branch `openlitespeed`). macOS arm64 (dev Mac)
  and Linux aarch64 (the 22.04 VM) built and passed every gate locally; **CI run 37207425654
  built all four targets green** (macos-15 arm64, macos-15-intel x86_64, ubuntu-22.04 x86_64,
  ubuntu-22.04-arm aarch64): minos 12.0 on both macOS slices, `NEEDED` glibc + libcrypt only and
  highest symbol `GLIBC_2.34` on both Linux arches, every probe served. Artifact = `bin/openlitespeed`, `conf/mime.properties`,
  `share/autoindex/`. Owner merges, then dispatches with publish on (build 1). The pins land in P2.
- **P2 — the app.** `core/openlitespeed.rs` (config generation per §4, loopback port range
  8400–8499 — disjoint from FrankenPHP's 8200s and Apache's 8300s so a switch never collides,
  recorded in `sites.override_port` like the others), `OverrideKind::Openlitespeed` (one arm, as
  `docs/ARCHITECTURE.md` promised), `binaries.rs` pins + `is_self_distributed`, the UI unlock
  (`NewSiteDialog`, `SiteDetail` already know the enum), the Windows refusal sentence in
  `platform/words.rs`, example `openlitespeed_site_check` (sandbox tier, the §1 probes against a
  fixture-owned root, `common::Reaped`), ledger rows (admin never exposed; loopback only; cache
  dir fixture-owned), `docs/SMOKE-TEST.md` rows for macOS and Linux, `docs/PORTS.md`, `docs/MAP.md`,
  `docs/ARCHITECTURE.md` §override, `THIRD-PARTY-NOTICES.md`.
  **State, 4 Oct 2026: shipped.** What P2 measured and decided, beyond §4:
  - **One file**: the vhost is INLINE in `httpd_config.conf` (measured to work), so the
    reconcile's config diff covers every input, as Apache's single conf does.
  - **Per-site server root** `<app-data>/openlitespeed/<domain>/` via `LSWS_HOME` (OpenLiteSpeed
    reads it before falling back to argv[0]); the binary stays shared in the cache.
  - **`user <uid>`** from `ProcessSupervisor::service_account` (OLS refuses to start without a
    resolvable `user`, even as non-root; a numeric uid is accepted).
  - **Ownership**: argv[0] is overwritten; the app-data marker passed twice after `-d`.
  - **Env vars solved** (the §4 open item): rewrite `E='NAME:value'` flags reach PHP as request
    params; `\` and `%` escaped; a value with both quote characters refused at save and switch.
  - **`.htaccess` precedence**: OpenLiteSpeed runs vhost rules first, so the vhost's front
    controller steps aside when the docroot has a root `.htaccess` (ledger #778).
- **P3 — the LSCache plugin end-to-end on a WordPress site.** **Shipped 4 Oct 2026**, as the
  SMOKE row it was planned as, on both VMs: the plugin activates (and rewrites `.htaccess` once —
  one watchdog restart, no churn: mtime measured stable over a minute of front and admin
  requests); a logged-out post goes `miss → hit`; an edit through the REST API with the block
  editor's nonce purges it (`miss`, new content, then `hit`). A wp-cli edit does not purge —
  `X-LiteSpeed-Purge` rides an HTTP response. What the running-app runs found and fixed on
  the way (ledger #783–#785): OLS never re-reads `.htaccess`; `-d` died with the app; an
  adopted session lost env vars.

## 7. Where the prototype lives

Built in the session scratchpad and copied to `~/PhpstormProjects/rexenv-ols-proto/`: the arm64
binary, `build-ols-macos.sh`, `err_data_generate.py`, the working `proto/` server root
(`conf/httpd_config.conf`, `conf/vhosts/site/vhconf.conf`, `www/`, `run.sh`). Not in this
repo: binaries live in `rexenv/runtimes`, and the recipe above is the source of truth until the
workflow lands there.
