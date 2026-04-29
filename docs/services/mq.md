# Amazon MQ

Broker, broker-user, and configuration metadata for Amazon MQ (ActiveMQ / RabbitMQ flavors). The emulator never spins up a real broker — `BrokerState` flips to `RUNNING` immediately after `CreateBroker`, and `DescribeBroker` synthesizes a plausible endpoint URL.

**Endpoint:** `http://localhost:4566`
**Signing name:** `mq`
**Protocol:** REST-JSON

## Operations

| Group | Operations |
|-------|-----------|
| Brokers | `CreateBroker`, `DescribeBroker`, `ListBrokers`, `UpdateBroker`, `DeleteBroker`, `RebootBroker` |
| Users | `CreateUser`, `DescribeUser`, `ListUsers`, `UpdateUser`, `DeleteUser` |
| Configurations | `CreateConfiguration`, `DescribeConfiguration`, `ListConfigurations` |

## Behavior notes

- `CreateBroker` rejects with `ConflictException` if another broker already uses the same `BrokerName`.
- `Users[]` passed to `CreateBroker` are inserted into the user store in one shot; subsequent `CreateUser` requests behave the same way.
- `DeleteBroker` cascades to delete every user attached to the broker.
- `DescribeBroker` synthesizes a fake `Endpoints[]` and `ConsoleURL` so client code that displays the broker URL works.
- `RebootBroker` is a no-op that returns `RUNNING`.
