# HTTPS / TLS

Some AWS code paths hard-require an `https://` endpoint - Cognito
hosted UI, S3 transfer acceleration, the Java SDK's CRT client,
several browser SDKs - and most teams want a green-padlock dev
experience anyway. AWSim ships a publicly-trusted TLS listener
out of the box, modelled on LocalStack's
`localhost.localstack.cloud` pattern.

## TL;DR

```bash
awsim --https-port 4567
# or
docker run -p 4566:4566 -p 4567:4567 \
  -e AWSIM_HTTPS_PORT=4567 \
  ghcr.io/qaidvoid/awsim:latest
```

Hit `https://aws.qaidvoid.dev:4567/_awsim/health` from a browser /
curl / SDK. No `--cacert`, no `NODE_EXTRA_CA_CERTS`, no
`AWS_CA_BUNDLE`, no system-trust-store install. `*.aws.qaidvoid.dev`
is also covered, so virtual-hosted URLs like
`https://s3.aws.qaidvoid.dev:4567/...` validate under the same cert.

## How it works

The DNS records for `aws.qaidvoid.dev` (and `*.aws.qaidvoid.dev`)
are `A 127.0.0.1` / `AAAA ::1`. Every request resolves back to your
own machine before any TLS handshake happens, so a connection to
`https://aws.qaidvoid.dev:4567` is identical to
`https://localhost:4567` *except* the SNI hostname matches a real
Let's Encrypt-issued cert that the system trust store already
trusts. Browsers and SDKs see a green padlock. Traffic never leaves
loopback.

The cert + matching private key are bundled into the awsim binary
via `include_bytes!` in
`crates/awsim/src/tls.rs` (sourced from
`crates/awsim/assets/aws.qaidvoid.dev/`). Distributing the private
key publicly is operationally safe because the DNS record is locked
to loopback - there is no remote victim an attacker could MITM.

## Which hostname should I use?

`localhost` and `aws.qaidvoid.dev` both resolve to your own machine
and both validate under the shipped cert, so the choice is about
which one fits the task.

Use `localhost` for everyday SDK and CLI work. It has no DNS
dependency, works offline, and is exempt from the `.dev` HSTS rule,
so plain HTTP on `:4566` stays available for quick calls. Most SDKs
also treat `localhost` and `127.0.0.1` specially: they bypass proxies
and default to S3 path-style addressing.

Use `aws.qaidvoid.dev` when you need real domain semantics: browser
flows such as the Cognito hosted UI or an S3 static website,
service-prefixed hostnames like `https://s3.aws.qaidvoid.dev:4567`, or
config that should mirror real AWS endpoints.

The wildcard is only one level deep, so virtual-hosted S3 buckets
(`bucket.s3.aws.qaidvoid.dev`) do not validate under the cert. Use
path-style addressing (`s3.aws.qaidvoid.dev/bucket`), which the SDKs
default to for custom endpoints. If you genuinely need virtual-hosted
buckets over TLS, reissue the cert with `*.s3.aws.qaidvoid.dev` added.

## Cert sources, in order of preference

When `--https-port` is set, AWSim picks a cert source like this:

1. **BYO** - if both `--tls-cert <path>` and `--tls-key <path>`
   (env: `AWSIM_TLS_CERT`, `AWSIM_TLS_KEY`) are provided, those PEMs
   win. Useful if you have a corporate CA installed on team
   machines, or want to point at a `mkcert`-issued local cert.
2. **Bundled** - the publicly-trusted `aws.qaidvoid.dev` cert
   compiled into the binary. This is the default upstream
   behaviour.
3. **Self-signed** - a fallback for forks that strip
   `crates/awsim/assets/aws.qaidvoid.dev/` from the build. AWSim
   mints a self-signed cert for `localhost` + `*.localhost` on
   first boot, caches it under `--tls-cache-dir` (default
   `<data-dir>/tls` or `$XDG_CACHE_HOME/awsim/tls`), and reuses it
   across restarts. Browsers will warn; SDKs need
   `AWS_CA_BUNDLE=<path>` or `NODE_EXTRA_CA_CERTS=<path>`.

## Bring your own cert with mkcert

The bundled cert is convenient, but every install shares the same
private key, so Let's Encrypt can revoke it (see Troubleshooting). To
avoid that entirely, issue your own locally-trusted cert with
[mkcert](https://github.com/FiloSottile/mkcert). The key never leaves
your machine and nothing can revoke it.

```bash
# 1. Trust a local CA in the system and browser stores
mkcert -install

# 2. Issue a cert covering both hostnames you might use
mkcert -cert-file fullchain.pem -key-file privkey.pem \
  aws.qaidvoid.dev "*.aws.qaidvoid.dev" localhost 127.0.0.1 ::1

# 3. Point awsim at it
awsim --https-port 4567 \
  --tls-cert ./fullchain.pem --tls-key ./privkey.pem
```

For SDKs and CLIs, point the trust knob at the mkcert root:

```bash
export AWS_CA_BUNDLE=$(mkcert -CAROOT)/rootCA.pem        # boto3 / aws-cli
export NODE_EXTRA_CA_CERTS=$(mkcert -CAROOT)/rootCA.pem  # node SDK
```

### Firefox with an XDG profile

`mkcert -install` only writes to the legacy `~/.mozilla/firefox`
path. Some distros, Gentoo for example, keep Firefox profiles under
`~/.config/mozilla/firefox` instead, so the CA lands in a store
Firefox never reads and pages fail with `SEC_ERROR_UNKNOWN_ISSUER`.
Add the CA to each real profile by hand, then restart Firefox fully
since it reads `cert9.db` only at startup:

```bash
CA="$(mkcert -CAROOT)/rootCA.pem"
for db in ~/.config/mozilla/firefox/*.default-release; do
  certutil -A -d "sql:$db" -n "mkcert development CA" -t "C,," -i "$CA"
done
```

Repeat the `certutil` line for any other profile directory you
actually use.

## CLI flags / env vars

| flag | env | default | meaning |
| --- | --- | --- | --- |
| `--https-port` | `AWSIM_HTTPS_PORT` | (off) | Enable HTTPS listener on this port. Both HTTP and HTTPS run side-by-side on different ports. |
| `--tls-cert` | `AWSIM_TLS_CERT` | (none) | BYO cert PEM. Requires `--tls-key`. |
| `--tls-key` | `AWSIM_TLS_KEY` | (none) | BYO key PEM. Requires `--tls-cert`. |
| `--tls-cache-dir` | `AWSIM_TLS_CACHE_DIR` | `<data-dir>/tls` or `$XDG_CACHE_HOME/awsim/tls` | Where AWSim materialises bundled / self-signed PEMs on disk. |

## /_awsim/tls endpoint

When HTTPS is enabled, awsim exposes `GET /_awsim/tls` so bootstrap
tooling can wire client trust automatically:

```json
{
  "https_port": 4567,
  "cert_path": "/abs/path/awsim-bundled-cert.pem",
  "public_trust": true,
  "domain": "aws.qaidvoid.dev"
}
```

`public_trust: true` means the cert chains to a publicly-trusted CA
and clients can skip `AWS_CA_BUNDLE` / `NODE_EXTRA_CA_CERTS` entirely.
`public_trust: false` (self-signed / BYO) means the consumer should
read `cert_path` and inject it into whatever trust knob the SDK
needs. Returns `404` when HTTPS is off.

## Troubleshooting

### Firefox: "can't add an exception" / HSTS warning

`.dev` is on the browser HSTS preload list, so every `*.dev` host is
HTTPS-only at the browser level and the usual "proceed anyway" escape
hatch is disabled by design. This does not mean anything is wrong. It
means you must reach awsim over a working HTTPS listener. Confirm you
started awsim with `--https-port` and that you are visiting the
`https://` URL on that port.

### SEC_ERROR_REVOKED_CERTIFICATE

The bundled cert was revoked. Because its private key ships in the
public binary, Let's Encrypt treats the key as compromised and
revokes the cert, and Firefox enforces that through its CRLite set
even before the cert expires. curl does not check revocation by
default, so it can still succeed while the browser refuses. Upgrade to
a build with a freshly issued cert, or switch to your own mkcert cert
using the steps above.

### SEC_ERROR_UNKNOWN_ISSUER with a mkcert cert

The mkcert CA is not in the trust store your browser actually reads.
The most common cause is an XDG Firefox profile under
`~/.config/mozilla/firefox`. See the Firefox XDG profile note above
for the `certutil` fix. For SDKs, set `AWS_CA_BUNDLE` or
`NODE_EXTRA_CA_CERTS` to `$(mkcert -CAROOT)/rootCA.pem`.

### dig returns nothing for aws.qaidvoid.dev

A local resolver is dropping the loopback answer. This is DNS
rebinding protection, covered in the Caveats below.

## Caveats

- **DNS rebinding protection**. Some local resolvers (Pi-hole,
  dnsmasq with `stop-dns-rebind`, certain corporate networks)
  refuse public-DNS responses that point to RFC1918 / loopback
  addresses. Symptom: `dig aws.qaidvoid.dev` returns nothing while
  `nslookup aws.qaidvoid.dev 1.1.1.1` returns `127.0.0.1`. Fix:
  add `127.0.0.1 aws.qaidvoid.dev` to `/etc/hosts` (or the Windows
  equivalent), or whitelist the zone in the resolver.
- **Renewal and revocation**. The bundled cert has a 90-day Let's
  Encrypt lifetime, and awsim releases ship a freshly issued cert on
  every cut. Two things can make a pinned build start failing. The
  cert expires if you do not upgrade for about three months. It can
  also be revoked at any time, because the private key is public (see
  the next point). In both cases the fix is to upgrade to a newer
  build or switch to your own mkcert cert.
- **Private-key exposure**. The bundled key is compiled into the
  binary and visible in the source repo
  (`crates/awsim/assets/aws.qaidvoid.dev/key.pem`) and on
  [crt.sh](https://crt.sh). No remote victim can be MITM'd because
  DNS resolves only to loopback, so the exposure itself is
  operationally inert. The practical consequence is revocation. Let's
  Encrypt treats a published key as compromised, so the cert can be
  revoked and browsers will reject it with
  `SEC_ERROR_REVOKED_CERTIFICATE`. Use mkcert if you want a cert that
  cannot be revoked out from under you.

## Forking awsim

Forks that don't want to ship `aws.qaidvoid.dev`-branded URLs can
delete the `crates/awsim/assets/aws.qaidvoid.dev/` directory. The
build script auto-detects its absence and the binary falls back to
the self-signed `localhost` cert with no other code changes
needed.

To swap in your own publicly-trusted domain instead, drop a
matching `cert.pem` + `key.pem` into the same path before building,
and update `BUNDLED_DOMAIN` in `crates/awsim/src/tls.rs` (a single
const string).
