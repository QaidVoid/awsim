# KMS

AWS Key Management Service for creating and controlling cryptographic keys.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `kms` |
| Persistence | No |

## Operations

### Key Lifecycle
- `CreateKey` — create a new KMS key (symmetric or asymmetric)
- `DescribeKey` — get metadata for a key or alias
- `ListKeys` — list all keys in the account/region
- `EnableKey` — enable a disabled key
- `DisableKey` — disable an active key
- `ScheduleKeyDeletion` — schedule a key for deletion with a waiting period

### Aliases
- `CreateAlias` — create a friendly alias for a key (must start with `alias/`)
- `DeleteAlias` — delete an alias
- `ListAliases` — list aliases, optionally filtered by key ID

### Cryptographic Operations
- `Encrypt` — encrypt plaintext using a KMS key
- `Decrypt` — decrypt ciphertext (key is resolved from ciphertext blob)
- `GenerateDataKey` — generate a data key for envelope encryption (returns plaintext + ciphertext)
- `GenerateDataKeyWithoutPlaintext` — generate a data key, return only the encrypted form
- `ReEncrypt` — decrypt ciphertext and re-encrypt under a different key

## Example

```bash
# Create a symmetric encryption key
aws --endpoint-url http://localhost:4567 \
  kms create-key \
  --key-spec SYMMETRIC_DEFAULT \
  --key-usage ENCRYPT_DECRYPT \
  --description "My app encryption key"

# Create an alias
aws --endpoint-url http://localhost:4567 \
  kms create-alias \
  --alias-name alias/my-app-key \
  --target-key-id <key-id>

# Encrypt data
aws --endpoint-url http://localhost:4567 \
  kms encrypt \
  --key-id alias/my-app-key \
  --plaintext "SGVsbG8gV29ybGQ="

# Generate a data key
aws --endpoint-url http://localhost:4567 \
  kms generate-data-key \
  --key-id alias/my-app-key \
  --key-spec AES_256
```

## Notes

- Encrypt/Decrypt operations use real AES-GCM symmetric encryption internally, so roundtrips work correctly.
- The ciphertext blob encodes the key ID, allowing `Decrypt` to resolve the key automatically.
- Key material is in-memory only and lost on restart (no persistence).
- `ScheduleKeyDeletion` marks the key as pending deletion but does not remove it from the store during the window.
