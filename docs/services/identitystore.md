# AWS Identity Store

Users, groups, and group memberships scoped by `IdentityStoreId`. Pairs with SSO Admin (permission sets and account assignments) for full IAM Identity Center coverage.

**Endpoint:** `http://localhost:4566`
**Signing name:** `identitystore`
**Protocol:** AWS-JSON 1.1 (X-Amz-Target prefix: `AWSIdentityStore`)

## Operations

| Group | Operations |
|-------|-----------|
| Users | `CreateUser`, `DescribeUser`, `GetUserId`, `ListUsers`, `UpdateUser`, `DeleteUser` |
| Groups | `CreateGroup`, `DescribeGroup`, `ListGroups`, `UpdateGroup`, `DeleteGroup` |
| Memberships | `CreateGroupMembership`, `DescribeGroupMembership`, `ListGroupMemberships`, `ListGroupMembershipsForMember`, `DeleteGroupMembership` |

## Behavior notes

- All resources are scoped by `IdentityStoreId`; pass any string (e.g. `d-1234567890`) — the emulator does not enforce a directory existing.
- `GetUserId` resolves a user by `AlternateIdentifier.UniqueAttribute.AttributeValue`, matching against `UserName`.
- `DeleteUser` cascades to remove every group membership the user belongs to. `DeleteGroup` cascades to remove every membership in the group.
- `UpdateUser` / `UpdateGroup` accept the SCIM-style `Operations` array and patch `displayName`, `title`, `userType`, and `description` fields.
