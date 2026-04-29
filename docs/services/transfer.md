# AWS Transfer Family

Server, user, and SSH-public-key metadata for Transfer Family. The emulator never spins up an actual SFTP/FTP listener — server `State` flips to `ONLINE` immediately after `CreateServer`.

**Endpoint:** `http://localhost:4566`
**Signing name:** `transfer`
**Protocol:** AWS-JSON 1.1 (X-Amz-Target prefix: `TransferService`)

## Operations

| Group | Operations |
|-------|-----------|
| Servers | `CreateServer`, `DescribeServer`, `ListServers`, `DeleteServer`, `StartServer`, `StopServer` |
| Users | `CreateUser`, `DescribeUser`, `ListUsers`, `UpdateUser`, `DeleteUser` |
| SSH keys | `ImportSshPublicKey`, `DeleteSshPublicKey` |

## Behavior notes

- `CreateServer` defaults: `Protocols: ["SFTP"]`, `IdentityProviderType: SERVICE_MANAGED`, `EndpointType: PUBLIC`, `Domain: S3`.
- `DeleteServer` cascades to delete every user and SSH key attached to the server.
- `DeleteUser` cascades to delete the user's SSH public keys.
- `Server.UserCount` and `User.SshPublicKeyCount` are kept in sync as resources come and go.
- `StartServer` / `StopServer` flip `State` to `ONLINE` / `OFFLINE` immediately.
