---
layout: home
hero:
  name: AWSim
  text: Fully Offline AWS Dev Environment
  tagline: Single binary. 60+ services. No auth tokens. No paywalls. No cloud dependency.
  actions:
    - theme: brand
      text: Get Started
      link: /guide/getting-started
    - theme: alt
      text: View on GitHub
      link: https://github.com/QaidVoid/awsim

features:
  - title: Fully Offline
    details: Zero network calls. All state management, crypto, and parsing is local. No internet required.
  - title: 60+ AWS Services
    details: S3, DynamoDB, SQS, Lambda, Cognito, IAM, EC2, RDS, and 50+ more — all in a single binary under 30 MB.
  - title: Sub-Second Startup
    details: Cold start in under 500ms with less than 10 MiB idle memory. No Docker required.
  - title: Admin Console
    details: SvelteKit-based web UI for browsing and managing all emulated resources.
  - title: Real Lambda Execution
    details: Actually runs your Node.js and Python Lambda functions as local processes.
  - title: State Persistence
    details: Optional snapshot-based persistence. Your resources survive restarts.
  - title: IAM Policy Enforcement
    details: Real policy evaluation engine — identity / resource / SCP / boundary / session policies, all 26 condition operators.
  - title: Estimated Billing
    details: Real-time rolling AWS bill from metered usage × vendored AWS pricing. Per-service breakdown, cost trajectory chart, and a "time to bankruptcy" widget.
---
