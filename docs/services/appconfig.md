# AWS AppConfig

Two services in one crate:

- **AppConfig** (control plane, signing name `appconfig`) — applications, environments, configuration profiles, hosted versions, deployments, deployment strategies.
- **AppConfigData** (data plane, signing name `appconfig`, service name `appconfigdata`) — `StartConfigurationSession` + `GetLatestConfiguration` for runtime polling.

Both services share the same backing state.

**Endpoint:** `http://localhost:4566`
**Protocol:** REST-JSON

## Control-plane operations

| Group | Operations |
|-------|-----------|
| Applications | `CreateApplication`, `GetApplication`, `ListApplications`, `UpdateApplication`, `DeleteApplication` |
| Environments | `CreateEnvironment`, `GetEnvironment`, `ListEnvironments`, `DeleteEnvironment` |
| Configuration profiles | `CreateConfigurationProfile`, `GetConfigurationProfile`, `ListConfigurationProfiles`, `DeleteConfigurationProfile` |
| Hosted versions | `CreateHostedConfigurationVersion`, `GetHostedConfigurationVersion` |
| Deployments | `StartDeployment`, `GetDeployment`, `ListDeployments` |
| Deployment strategies | `CreateDeploymentStrategy`, `ListDeploymentStrategies` |

## Data-plane operations

| Operation | Method / Path |
|-----------|--------------|
| `StartConfigurationSession` | `POST /configurationsessions` |
| `GetLatestConfiguration` | `GET /configuration` |

## Behavior notes

- Application/environment/profile IDs are 7-character hex; resources cascade-delete when the parent is removed (deleting an application removes its environments, profiles, hosted versions, and deployments).
- `CreateHostedConfigurationVersion` increments the parent profile's `LatestVersionNumber` automatically — pass any `VersionLabel` you want surfaced via `GetLatestConfiguration`.
- `StartDeployment` is fast-forwarded — the returned deployment is already `State: COMPLETE` with `PercentageComplete: 100`. The event log records a single `DEPLOYMENT_COMPLETED` entry.
- `ListDeploymentStrategies` always includes the AWS-managed predefined strategies (`AppConfig.AllAtOnce`, `AppConfig.Linear50PercentEvery30Seconds`, `AppConfig.Canary10Percent20Minutes`).
- `StartConfigurationSession` accepts either resource IDs or names for `ApplicationIdentifier` / `EnvironmentIdentifier` / `ConfigurationProfileIdentifier`.
- `GetLatestConfiguration` rotates the polling token on each call (the SDK stores the new token for the next poll). `Configuration` is a base64-encoded blob; if no hosted version exists, the response is empty.
