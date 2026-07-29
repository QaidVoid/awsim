#!/usr/bin/env python3
"""Compare AWSim's response field names against the AWS SDK models.

Every service model names its members twice: the Rust-facing name a
handler is likely to use, and the `locationName` that actually goes on
the wire. Where the two differ, a handler that emits the model name
produces a response the SDK silently parses as empty. That failure is
invisible from inside the codebase, which is why it kept shipping.

This script does no network calls. It reads the botocore models bundled
with the AWS CLI, collects every output member whose wire name differs
from its model name, then greps the AWSim sources for handlers emitting
the model name instead. Report only, no changes.

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
        renames = renamed_output_members(model)
        if not renames:
            continue

        files = sources(src_dir)
        for op, pairs in sorted(renames.items()):
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

    print(f"checked {checked} services against the SDK models")
    if not findings:
        print("no output members emitted under a model name that differs on the wire")
        return 0

    for service in sorted(findings):
        print(f"\n{service}:")
        for line in sorted(set(findings[service])):
            print(f"  {line}")
    total = sum(len(set(v)) for v in findings.values())
    print(f"\n{total} suspect field(s) across {len(findings)} service(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
