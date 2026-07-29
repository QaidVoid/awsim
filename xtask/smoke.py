#!/usr/bin/env python3
"""Start the built binary and send every registered service a request.

This exists because of a real failure: a build once set hyper's
`header_read_timeout` without registering a timer, panicking the tokio
worker on the very first request. Every in-tree Rust test passed,
because none of them starts the binary and talks to it over a socket.

The target list comes from the running server's own service registry, so
a newly registered service is covered without editing this file.

Usage:
    python3 xtask/smoke.py path/to/awsim
"""

import http.client
import json
import subprocess
import sys
import time


def wait_for_health(port, proc, timeout=60):
    """Wait until the server answers, or explain why it never did."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        if proc.poll() is not None:
            out, err = proc.communicate()
            sys.exit(
                f"server exited before serving (status {proc.returncode})\n"
                f"{err.decode(errors='replace')}"
            )
        try:
            conn = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
            conn.request("GET", "/_awsim/health")
            if conn.getresponse().status == 200:
                conn.close()
                return
            conn.close()
        except OSError:
            pass
        time.sleep(0.3)
    sys.exit(f"server did not become healthy on port {port} within {timeout}s")


def registered_services(port):
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    conn.request("GET", "/_awsim/services")
    resp = conn.getresponse()
    if resp.status != 200:
        sys.exit(f"could not list services: HTTP {resp.status}")
    services = json.load(resp)["services"]
    conn.close()
    return services


def probe(port, service):
    """Send one request a service should recognise as addressed to it.

    The assertion is deliberately weak: that a well-formed HTTP response
    comes back at all. A service legitimately rejecting a bogus operation
    is fine; a panicked worker, a hang, or a connection reset is not.
    Depth of coverage belongs in the conformance harness, not here.

    501 counts as a pass. It is AWSim's deliberate "this AWS operation
    exists but is not built yet" signal, which is information rather than
    breakage. Any other 5xx is a failure.
    """
    signing = service["signingName"]
    protocol = service["protocol"]
    auth = (
        "AWS4-HMAC-SHA256 "
        f"Credential=test/20260101/us-east-1/{signing}/aws4_request"
    )

    if protocol in ("AwsJson1_0", "AwsJson1_1"):
        version = "1.0" if protocol == "AwsJson1_0" else "1.1"
        headers = {
            "Content-Type": f"application/x-amz-json-{version}",
            "X-Amz-Target": f"{signing}.AwsimSmokeProbe",
            "Authorization": auth,
        }
        return "POST", "/", "{}", headers

    if protocol in ("AwsQuery", "Ec2Query"):
        headers = {
            "Content-Type": "application/x-www-form-urlencoded",
            "Authorization": auth,
        }
        return "POST", "/", "Action=AwsimSmokeProbe&Version=2011-06-15", headers

    # REST services: a GET at the root of the service is enough to prove
    # the connection and router are alive.
    return "GET", "/", None, {"Authorization": auth}


def main():
    if len(sys.argv) < 2:
        sys.exit("usage: smoke.py path/to/awsim")
    binary = sys.argv[1]
    port = 4599

    proc = subprocess.Popen(
        [binary, "--port", str(port), "-v", "warn"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        wait_for_health(port, proc)
        services = registered_services(port)
        print(f"probing {len(services)} registered services on port {port}")

        failures = []
        for service in sorted(services, key=lambda s: s["signingName"]):
            name = service["signingName"]
            method, path, body, headers = probe(port, service)
            try:
                conn = http.client.HTTPConnection("127.0.0.1", port, timeout=15)
                conn.request(method, path, body, headers)
                resp = conn.getresponse()
                resp.read()
                conn.close()
                if resp.status >= 500 and resp.status != 501:
                    failures.append(f"{name}: HTTP {resp.status}")
            except Exception as exc:
                failures.append(f"{name}: {type(exc).__name__}: {exc}")

            if proc.poll() is not None:
                failures.append(f"{name}: server died while handling this request")
                break

        if failures:
            print(f"\n{len(failures)} service(s) failed their smoke probe:")
            for f in failures:
                print(f"  {f}")
            sys.exit(1)

        print(f"all {len(services)} services responded")
    finally:
        proc.kill()
        proc.wait()


if __name__ == "__main__":
    main()
