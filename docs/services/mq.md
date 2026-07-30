# Amazon MQ

Broker, broker-user, and configuration metadata for Amazon MQ (ActiveMQ and
RabbitMQ flavors). The emulator never spins up a real broker: a new one
reaches `RUNNING` without provisioning anything, and `DescribeBroker`
synthesizes a plausible endpoint URL.

**Endpoint:** `http://localhost:4566`
**Signing name:** `mq`
**Protocol:** REST-JSON
**Persistence:** Yes

Amazon MQ names every request and response field in camelCase
(`brokerName`, `engineType`, `hostInstanceType`), unlike most other AWS
services. The SDK models expose them PascalCased, so the AWS CLI and the
SDKs handle the difference for you, but a hand-rolled `curl` has to use
the camelCase spelling.

## Operations

| Group | Operations |
|-------|-----------|
| Brokers | `CreateBroker`, `DescribeBroker`, `ListBrokers`, `UpdateBroker`, `DeleteBroker`, `RebootBroker` |
| Users | `CreateUser`, `DescribeUser`, `ListUsers`, `UpdateUser`, `DeleteUser` |
| Configurations | `CreateConfiguration`, `DescribeConfiguration`, `ListConfigurations` |

## Behavior notes

- `CreateBroker` rejects with `ConflictException` if another broker already uses the same `brokerName`.
- `users[]` passed to `CreateBroker` are inserted into the user store in one shot; subsequent `CreateUser` requests behave the same way.
- `DeleteBroker` cascades to delete every user attached to the broker.
- `DescribeBroker` synthesizes a fake `brokerInstances[]` with endpoints and a console URL so client code that displays the broker URL works.
- `UpdateBroker` stages its changes rather than applying them, exactly as AWS does. The staged values show up as `pending*` fields on `DescribeBroker` and only become live on the next `RebootBroker`.
- `RebootBroker` promotes those staged values and puts the broker through `REBOOT_IN_PROGRESS` for a couple of seconds before it settles back to `RUNNING`, so a poll loop that waits for the reboot has something to observe.
- A new broker settles into `RUNNING` immediately. Set `AWSIM_MQ_CREATE_DELAY_SECS` to hold it in `CREATION_IN_PROGRESS` for that many seconds if you need to exercise the transitional path.
- `logs` on `DescribeBroker` is the derived summary AWS returns, carrying the CloudWatch log group names, rather than the raw toggles the broker was created with.
