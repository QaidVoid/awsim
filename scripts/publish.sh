#!/usr/bin/env bash
# Publish all workspace crates to crates.io in dependency order, sleeping
# between new-crate uploads to respect the 1-per-10-minute rate limit.
#
# Idempotent: already-published crate versions are detected and skipped
# without sleeping. Re-run after rate-limit timeouts to resume from where
# you left off. Override the wait via WAIT=<seconds>.

set -u
WAIT="${WAIT:-600}"

CRATES=(
    # Level 0 — no internal deps
    awsim-iam-policy
    awsim-core
    # Level 1 — depend only on level 0
    awsim-acm
    awsim-apigateway
    awsim-appsync
    awsim-athena
    awsim-batch
    awsim-bedrock
    awsim-cloudformation
    awsim-cloudfront
    awsim-cloudtrail
    awsim-cloudwatch-logs
    awsim-cloudwatch-metrics
    awsim-cognito
    awsim-comprehend
    awsim-datasync
    awsim-dynamodb
    awsim-ec2
    awsim-ecr
    awsim-ecs
    awsim-eks
    awsim-elb
    awsim-eventbridge
    awsim-firehose
    awsim-glue
    awsim-kendra
    awsim-kinesis
    awsim-kms
    awsim-opensearch
    awsim-organizations
    awsim-polly
    awsim-rds
    awsim-route53
    awsim-s3
    awsim-scheduler
    awsim-secretsmanager
    awsim-ses
    awsim-sns
    awsim-sqs
    awsim-ssm
    awsim-sso-admin
    awsim-stepfunctions
    awsim-sts
    awsim-waf
    # Level 2 — depend on level 1
    awsim-iam
    awsim-lambda
    # Binary
    awsim
)

format_eta() {
    local secs=$1
    local h=$((secs / 3600))
    local m=$(((secs % 3600) / 60))
    printf "%dh %dm" "$h" "$m"
}

total=${#CRATES[@]}
i=0
published=0

for crate in "${CRATES[@]}"; do
    i=$((i + 1))
    echo ""
    echo "================================================================="
    echo "[$i/$total] $crate"
    echo "================================================================="

    while true; do
        out=$(cargo publish -p "$crate" --allow-dirty 2>&1)
        ec=$?
        echo "$out"

        if [ $ec -eq 0 ]; then
            published=$((published + 1))
            remaining=$((total - i))
            eta=$((remaining * WAIT))
            echo ""
            echo "✓ $crate published ($published new this session, $remaining crates remaining, ETA $(format_eta $eta))"
            if [ "$remaining" -gt 0 ]; then
                echo "  Sleeping ${WAIT}s for rate limit..."
                sleep "$WAIT"
            fi
            break
        fi

        if echo "$out" | grep -qE "already (uploaded|published)|crate version.*already"; then
            echo ""
            echo "ℹ $crate already on crates.io — skipping (no wait)."
            break
        fi

        if echo "$out" | grep -q "429 Too Many Requests"; then
            echo ""
            echo "⚠ Rate-limited on $crate. Sleeping ${WAIT}s before retry..."
            sleep "$WAIT"
            continue
        fi

        echo ""
        echo "✗ $crate failed with unrecoverable error. Stopping."
        echo "  Re-run this script after fixing to resume from $crate."
        exit 1
    done
done

echo ""
echo "================================================================="
echo "✓ All crates published. ($published new this session.)"
echo "================================================================="
