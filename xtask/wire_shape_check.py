#!/usr/bin/env python3
"""Compare AWSim's wire surface against the AWS SDK models.

Three checks, all static, all read-only:

1. Response members emitted under the SDK's model name where the wire
   name differs, which an SDK parses as absent.
2. XML list members emitted as a bare array where the protocol needs one
   element per item.
3. REST routes whose registered path does not match the model's
   `requestUri`, which makes the operation unreachable.

None of these are visible from inside the codebase: the handler and its
tests agree with each other, and both disagree with every real client.

Usage:
    python3 xtask/wire_shape_check.py [--models DIR] [--service NAME]
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import re
import sys
from collections import defaultdict

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# botocore's service directory name is not always AWSim's crate name.
CRATE_OVERRIDES = {
    "apigateway": "awsim-apigateway",
    "cloudwatch": "awsim-cloudwatch-metrics",
    "elbv2": "awsim-elb",
    "elasticloadbalancingv2": "awsim-elb",
    "efs": "awsim-efs",
    "elasticfilesystem": "awsim-efs",
    "sfn": "awsim-stepfunctions",
    "states": "awsim-stepfunctions",
    "secretsmanager": "awsim-secretsmanager",
    "cognito-idp": "awsim-cognito-idp",
    "cognito-identity": "awsim-cognito-identity",
    "rds-data": "awsim-rds-data",
    "resourcegroupstaggingapi": "awsim-resourcegroupstagging",
    "pinpoint": "awsim-pinpoint",
    "logs": "awsim-logs",
}


def find_models(explicit: str | None) -> str | None:
    if explicit:
        return explicit
    for pattern in (
        "/opt/aws-cli/v2/*/dist/awscli/botocore/data",
        "/usr/lib/python3*/site-packages/botocore/data",
        os.path.expanduser("~/.local/lib/python3*/site-packages/botocore/data"),
    ):
        hits = sorted(glob.glob(pattern))
        if hits:
            return hits[-1]
    return None


def renamed_output_members(model: dict) -> dict[str, list[tuple[str, str]]]:
    """Map operation -> [(model name, wire name)] for renamed members."""
    shapes = model.get("shapes", {})
    found: dict[str, list[tuple[str, str]]] = {}
    for op, spec in model.get("operations", {}).items():
        out = spec.get("output", {}).get("shape")
        if not out or out not in shapes:
            continue
        pairs = []
        for name, member in shapes[out].get("members", {}).items():
            wire = member.get("locationName")
            # A member bound to a header or the status code is not a body
            # field, so emitting the model name there is fine.
            if member.get("location") in ("header", "headers", "statusCode"):
                continue
            if wire and wire != name:
                pairs.append((name, wire))
        if pairs:
            found[op] = pairs
    return found


def unnested_lists(model: dict) -> dict[str, list[tuple[str, str]]]:
    """Map operation -> [(member, element tag)] for XML list members.

    In the XML protocols a list serializes as an outer element holding
    one inner element per item: `<Metrics><member>..</member></Metrics>`.
    Handlers build responses as JSON, where a bare array flattens into
    repeated outer elements instead, which clients read as one empty
    struct per item. Only non-flattened lists are reported; a flattened
    list really is repeated outer elements.
    """
    shapes = model.get("shapes", {})
    found: dict[str, list[tuple[str, str]]] = {}
    for op, spec in model.get("operations", {}).items():
        out = spec.get("output", {}).get("shape")
        if not out or out not in shapes:
            continue
        pairs = []
        for name, member in shapes[out].get("members", {}).items():
            target = shapes.get(member.get("shape"), {})
            if target.get("type") != "list" or target.get("flattened"):
                continue
            inner = target.get("member", {}).get("locationName", "member")
            pairs.append((member.get("locationName", name), inner))
        if pairs:
            found[op] = pairs
    return found


def xml_protocol(model: dict) -> bool:
    meta = model.get("metadata", {})
    protocol = meta.get("protocol") or ""
    protocols = meta.get("protocols") or []
    return protocol in ("query", "ec2", "rest-xml") or any(
        p in ("query", "ec2", "rest-xml") for p in protocols
    )


def rest_protocol(model: dict) -> bool:
    meta = model.get("metadata", {})
    protocol = meta.get("protocol") or ""
    protocols = meta.get("protocols") or []
    return protocol in ("rest-json", "rest-xml") or any(
        p in ("rest-json", "rest-xml") for p in protocols
    )


# Brace-matching the struct body does not work: a path pattern contains
# braces of its own (`/schedules/{Name}`), so the first `}` closes the
# match early. Read the two fields directly instead.
ROUTE_RE = re.compile(
    r'RouteDefinition\s*\{\s*method:\s*"(?P<method>[^"]*)"\s*,\s*'
    r'path_pattern:\s*"(?P<path>[^"]*)"',
    re.S,
)


def registered_routes(files: list[tuple[str, str]]) -> set[tuple[str, str]]:
    """Collect (method, path) for every RouteDefinition in a crate."""
    routes = set()
    for _, text in files:
        for m in ROUTE_RE.finditer(text):
            routes.add((m.group("method").upper(), m.group("path")))
    return routes


def normalize_path(path: str) -> str:
    """Reduce a path to its shape, so parameter names do not matter.

    A trailing slash is dropped because the router retries without one,
    which is what makes Route53's `/rrset/` and Backup's `/backup/plans/`
    reachable from the bare registration.
    """
    path = path.split("?", 1)[0]
    path = re.sub(r"\{[^}]*\+\}", "{+}", path)
    path = re.sub(r"\{[^}]*\}", "{}", path)
    return path.rstrip("/") or "/"


def route_matches(wanted: str, registered: str) -> bool:
    """Whether a registered pattern can serve the model's path."""
    if wanted == registered:
        return True
    # A greedy tail absorbs everything after it, so `/{}/{+}` serves any
    # deeper path rooted at the same prefix.
    if "{+}" not in registered:
        return False
    prefix = registered.split("{+}")[0]
    return wanted.startswith(prefix)


def route_mismatches(model: dict, files: list[tuple[str, str]]) -> list[str]:
    """Implemented operations whose model path no route can serve."""
    routes = {(m, normalize_path(p)) for m, p in registered_routes(files)}
    if not routes:
        return []
    findings = []
    for op, spec in sorted(model.get("operations", {}).items()):
        http = spec.get("http") or {}
        uri = http.get("requestUri")
        method = (http.get("method") or "").upper()
        if not uri or not method:
            continue
        # Only operations the crate actually dispatches are in scope; an
        # unimplemented one has no route by design.
        if not any(re.search(r'"%s"' % re.escape(op), text) for _, text in files):
            continue
        wanted = normalize_path(uri)
        if any(m == method and route_matches(wanted, p) for m, p in routes):
            continue
        findings.append(f'{op}: no route serves {method} "{wanted}"')
    return findings


def crate_dir(service: str) -> str | None:
    name = CRATE_OVERRIDES.get(service, f"awsim-{service}")
    path = os.path.join(REPO, "crates", name, "src")
    return path if os.path.isdir(path) else None


def sources(directory: str) -> list[tuple[str, str]]:
    """Read every .rs file, minus its test module.

    Test inputs legitimately use the model name for members that are only
    renamed on output, so including them produces false positives.
    """
    out = []
    for root, _, files in os.walk(directory):
        for f in sorted(files):
            if not f.endswith(".rs"):
                continue
            p = os.path.join(root, f)
            with open(p, encoding="utf-8", errors="replace") as fh:
                text = fh.read()
            cut = text.find("#[cfg(test)]")
            out.append((p, text[:cut] if cut != -1 else text))
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--models", help="botocore data directory")
    ap.add_argument("--service", help="check a single botocore service name")
    args = ap.parse_args()

    root = find_models(args.models)
    if not root:
        print("could not find the botocore model directory; pass --models", file=sys.stderr)
        return 2

    services = sorted(os.listdir(root))
    if args.service:
        services = [s for s in services if s == args.service]

    findings: dict[str, list[str]] = defaultdict(list)
    checked = 0

    for service in services:
        versions = sorted(glob.glob(os.path.join(root, service, "*", "service-2.json*")))
        if not versions:
            continue
        src_dir = crate_dir(service)
        if not src_dir:
            continue
        checked += 1
        with open(versions[-1], encoding="utf-8") as fh:
            model = json.load(fh)
        files = sources(src_dir)

        for op, pairs in sorted(renamed_output_members(model).items()):
            for model_name, wire_name in pairs:
                emits_model = re.compile(r'"%s"\s*:' % re.escape(model_name))
                emits_wire = re.compile(r'"%s"\s*:' % re.escape(wire_name))
                hits = [p for p, s in files if emits_model.search(s)]
                if not hits:
                    continue
                if any(emits_wire.search(s) for _, s in files):
                    continue
                rel = os.path.relpath(hits[0], REPO)
                findings[service].append(
                    f'{op}: emits "{model_name}", wire name is "{wire_name}" ({rel})'
                )

        if rest_protocol(model):
            for line in route_mismatches(model, files):
                findings[service].append(line)

        if not xml_protocol(model):
            continue
        for op, pairs in sorted(unnested_lists(model).items()):
            for outer, inner in pairs:
                # A nested list is emitted as `"Outer": { "Inner": .. }`.
                # A bare `"Outer": something_else` flattens on the wire.
                nested = re.compile(r'"%s"\s*:\s*\{\s*"%s"' % (re.escape(outer), re.escape(inner)))
                bare = re.compile(r'"%s"\s*:\s*(?!\{\s*"%s")' % (re.escape(outer), re.escape(inner)))
                hits = [p for p, s in files if bare.search(s)]
                if not hits or any(nested.search(s) for _, s in files):
                    continue
                rel = os.path.relpath(hits[0], REPO)
                findings[service].append(
                    f'{op}: "{outer}" is a list; each item needs its own '
                    f'<{inner}> element ({rel})'
                )

    print(f"checked {checked} services against the SDK models")
    if not findings:
        print("no wire-shape or route mismatches")
        return 0

    for service in sorted(findings):
        print(f"\n{service}:")
        for line in sorted(set(findings[service])):
            print(f"  {line}")
    total = sum(len(set(v)) for v in findings.values())
    print(f"\n{total} finding(s) across {len(findings)} service(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
