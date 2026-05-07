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

## Caveats

- **DNS rebinding protection**. Some local resolvers (Pi-hole,
  dnsmasq with `stop-dns-rebind`, certain corporate networks)
  refuse public-DNS responses that point to RFC1918 / loopback
  addresses. Symptom: `dig aws.qaidvoid.dev` returns nothing while
  `nslookup aws.qaidvoid.dev 1.1.1.1` returns `127.0.0.1`. Fix:
  add `127.0.0.1 aws.qaidvoid.dev` to `/etc/hosts` (or the Windows
  equivalent), or whitelist the zone in the resolver.
- **Renewal**. The bundled cert has a 90-day Let's Encrypt
  lifetime. Awsim releases include a freshly-renewed cert on every
  cut. If you pin to a specific tag and don't upgrade for ~3
  months you'll start getting expired-cert warnings; bump the tag.
- **Private-key exposure**. The bundled key is compiled into the
  binary and visible in the source repo
  (`crates/awsim/assets/aws.qaidvoid.dev/key.pem`) and on
  [crt.sh](https://crt.sh). This is operationally inert - DNS
  resolves only to loopback - but worth knowing if your threat
  model differs.

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
