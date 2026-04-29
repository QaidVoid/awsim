# Amazon Pinpoint

Apps (projects), endpoints, segments, and campaigns. The emulator never delivers a real message — campaigns land in `COMPLETED` immediately so callers don't have to poll.

**Endpoint:** `http://localhost:4566`
**Signing name:** `mobiletargeting`
**Protocol:** REST-JSON

## Operations

| Group | Operations |
|-------|-----------|
| Apps | `CreateApp`, `GetApp`, `GetApps`, `DeleteApp` |
| Endpoints | `UpdateEndpoint`, `GetEndpoint`, `DeleteEndpoint` |
| Segments | `CreateSegment`, `GetSegment`, `GetSegments`, `DeleteSegment` |
| Campaigns | `CreateCampaign`, `GetCampaign`, `GetCampaigns`, `DeleteCampaign` |

## Behavior notes

- `DeleteApp` cascades to delete every endpoint, segment, and campaign attached to the application.
- `UpdateEndpoint` is upsert-style — pass either an existing or new `EndpointId`.
- Campaign `State.CampaignStatus` is `COMPLETED` immediately after `CreateCampaign`. Real Pinpoint progresses through `PENDING_NEXT_RUN` / `EXECUTING` / etc. — that machinery is intentionally collapsed here.
- The data plane (`SendMessages`, `SendUsersMessages`) is not emulated.
