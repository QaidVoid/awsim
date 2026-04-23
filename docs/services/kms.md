# KMS

AWS Key Management Service for creating and controlling cryptographic keys used for encryption, decryption, and digital signatures.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `kms` |
| Target Prefix | `TrentService` |
| Persistence | No |

## Quick Start

Create a key, encrypt data, then decrypt it:

```bash
# Create a symmetric encryption key
KEY_ID=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: TrentService.CreateKey" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kms/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"KeySpec":"SYMMETRIC_DEFAULT","KeyUsage":"ENCRYPT_DECRYPT","Description":"My app key"}' \
  | jq -r '.KeyMetadata.KeyId')

echo "Key ID: $KEY_ID"

# Create an alias
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: TrentService.CreateAlias" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kms/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"AliasName\":\"alias/my-app-key\",\"TargetKeyId\":\"$KEY_ID\"}"

# Encrypt data (Plaintext must be base64-encoded)
CIPHERTEXT=$(curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: TrentService.Encrypt" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kms/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"KeyId\":\"alias/my-app-key\",\"Plaintext\":\"SGVsbG8gV29ybGQ=\"}" \
  | jq -r '.CiphertextBlob')

# Decrypt data (KeyId not needed — it's embedded in the ciphertext)
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: TrentService.Decrypt" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kms/aws4_request, SignedHeaders=host, Signature=fake" \
  -d "{\"CiphertextBlob\":\"$CIPHERTEXT\"}"
```

## Operations

### Key Lifecycle
- `CreateKey` — create a new KMS key
  - Input: `KeySpec` (`SYMMETRIC_DEFAULT`, `RSA_2048`, `RSA_3072`, `RSA_4096`, `ECC_NIST_P256`, `ECC_NIST_P384`, `HMAC_256`), `KeyUsage` (`ENCRYPT_DECRYPT`, `SIGN_VERIFY`, `GENERATE_VERIFY_MAC`), `Description`, `Tags`
  - Returns: `KeyMetadata` with `KeyId` (UUID), `Arn`, `KeyState: "Enabled"`, `KeySpec`, `KeyUsage`, `CreationDate`

- `DescribeKey` — get metadata for a key or alias
  - Input: `KeyId` (key ID, key ARN, or alias name like `alias/my-key`)
  - Returns: `KeyMetadata` (same as CreateKey response)

- `ListKeys` — list all keys in the account/region
  - Input: optional `Limit`, `Marker`
  - Returns: paginated `Keys` list with `KeyId` and `KeyArn`

- `EnableKey` — re-enable a disabled key
  - Input: `KeyId`

- `DisableKey` — disable a key (prevents encrypt/decrypt use)
  - Input: `KeyId`

- `ScheduleKeyDeletion` — schedule a key for deletion
  - Input: `KeyId`, `PendingWindowInDays` (7–30 days)
  - Returns: `KeyId`, `DeletionDate`
  - Sets key state to `PendingDeletion`

### Aliases
- `CreateAlias` — create a friendly alias for a key
  - Input: `AliasName` (must start with `alias/`, cannot start with `alias/aws/`), `TargetKeyId`

- `DeleteAlias` — delete an alias (does not delete the key)
  - Input: `AliasName`

- `ListAliases` — list aliases, optionally filtered by key ID
  - Input: optional `KeyId`, `Limit`, `Marker`
  - Returns: paginated `Aliases` list with `AliasName`, `AliasArn`, `TargetKeyId`

- `UpdateAlias` — point an existing alias to a different key (useful for key rotation)
  - Input: `AliasName`, `TargetKeyId`

- `UpdateKeyDescription` — update the description of a KMS key
  - Input: `KeyId`, `Description`

### Key Rotation
- `GetKeyRotationStatus` — check whether automatic key rotation is enabled for a key
  - Input: `KeyId`
  - Returns: `KeyRotationEnabled` (boolean)

- `EnableKeyRotation` — enable automatic annual key rotation for a symmetric key
  - Input: `KeyId`

- `DisableKeyRotation` — disable automatic key rotation for a key
  - Input: `KeyId`

### Grants
- `CreateGrant` — create a grant that allows a grantee to use a key
  - Input: `KeyId`, `GranteePrincipal`, `Operations` (list of allowed operations)
  - Returns: `GrantId`, `GrantToken`

- `ListGrants` — list grants on a key
  - Input: `KeyId`, optional `Limit`, `Marker`
  - Returns: paginated `Grants` list

- `ListRetirableGrants` — list grants where the given principal is the retiring principal
  - Input: `RetiringPrincipal`, optional `Limit`, `Marker`
  - Returns: paginated `Grants` list

- `RetireGrant` — retire a grant using the grant token or grant ID
  - Input: `GrantToken` or (`KeyId` + `GrantId`)

- `RevokeGrant` — revoke a grant immediately
  - Input: `KeyId`, `GrantId`

### Key Policies
- `GetKeyPolicy` — get the key policy document for a KMS key
  - Input: `KeyId`, `PolicyName` (must be `"default"`)
  - Returns: `Policy` (JSON string)

- `PutKeyPolicy` — set the key policy for a KMS key
  - Input: `KeyId`, `PolicyName` (must be `"default"`), `Policy` (JSON string)

- `ListKeyPolicies` — list policy names for a key (always returns `["default"]`)
  - Input: `KeyId`, optional `Limit`, `Marker`
  - Returns: `PolicyNames`

### Cryptographic Operations
- `Encrypt` — encrypt plaintext using a KMS key
  - Input: `KeyId`, `Plaintext` (base64-encoded bytes, max 4096 bytes)
  - Returns: `CiphertextBlob` (base64-encoded, includes key ID for decryption), `KeyId`, `EncryptionAlgorithm`

- `Decrypt` — decrypt ciphertext (key is resolved from ciphertext blob)
  - Input: `CiphertextBlob` (base64-encoded), optional `KeyId`, `EncryptionAlgorithm`
  - Returns: `Plaintext` (base64-encoded), `KeyId`, `EncryptionAlgorithm`

- `GenerateDataKey` — generate a data key for envelope encryption
  - Input: `KeyId`, `KeySpec` (`AES_128` or `AES_256`)
  - Returns: `CiphertextBlob` (encrypted data key), `Plaintext` (raw data key — use for encryption, then discard), `KeyId`

- `GenerateDataKeyWithoutPlaintext` — generate a data key, return only the encrypted form
  - Input: `KeyId`, `KeySpec`
  - Returns: `CiphertextBlob` only (no plaintext — for deferred decryption scenarios)

- `ReEncrypt` — decrypt ciphertext and re-encrypt under a different key
  - Input: `CiphertextBlob`, `DestinationKeyId`, optional `SourceKeyId`
  - Returns: new `CiphertextBlob` encrypted under `DestinationKeyId`

- `GenerateRandom` — generate cryptographically random bytes
  - Input: `NumberOfBytes` (1–1024)
  - Returns: `Plaintext` (base64-encoded random bytes)

## Curl Examples

```bash
# 1. Generate a data key for envelope encryption
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: TrentService.GenerateDataKey" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kms/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"KeyId":"alias/my-app-key","KeySpec":"AES_256"}'

# 2. List all aliases
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: TrentService.ListAliases" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kms/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{}'

# 3. Re-encrypt data under a new key
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: TrentService.ReEncrypt" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/kms/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"CiphertextBlob":"<base64-ciphertext>","DestinationKeyId":"<new-key-id>"}'
```

## SDK Example

```typescript
import {
  KMSClient,
  CreateKeyCommand,
  CreateAliasCommand,
  EncryptCommand,
  DecryptCommand,
  GenerateDataKeyCommand,
} from '@aws-sdk/client-kms';

const kms = new KMSClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create symmetric key
const { KeyMetadata } = await kms.send(new CreateKeyCommand({
  KeySpec: 'SYMMETRIC_DEFAULT',
  KeyUsage: 'ENCRYPT_DECRYPT',
  Description: 'My application encryption key',
}));

const keyId = KeyMetadata!.KeyId!;

// Create alias
await kms.send(new CreateAliasCommand({
  AliasName: 'alias/my-app-key',
  TargetKeyId: keyId,
}));

// Encrypt data
const plaintext = Buffer.from('Hello, World!');
const { CiphertextBlob } = await kms.send(new EncryptCommand({
  KeyId: 'alias/my-app-key',
  Plaintext: plaintext,
}));

console.log('Ciphertext length:', CiphertextBlob!.length);

// Decrypt (no KeyId needed — it's in the ciphertext)
const { Plaintext } = await kms.send(new DecryptCommand({
  CiphertextBlob,
}));

console.log('Decrypted:', Buffer.from(Plaintext!).toString()); // Hello, World!

// Envelope encryption: generate a data key
const { Plaintext: dataKey, CiphertextBlob: encryptedDataKey } = await kms.send(
  new GenerateDataKeyCommand({
    KeyId: keyId,
    KeySpec: 'AES_256',
  })
);

// Use dataKey to encrypt your data locally, then discard dataKey
// Store encryptedDataKey alongside your encrypted data
console.log('Data key length:', dataKey!.length); // 32 bytes for AES_256
```

## Behavior Notes

- KMS uses AES-GCM symmetric encryption internally, so `Encrypt`/`Decrypt` roundtrips work correctly.
- The ciphertext blob embeds the key ID so `Decrypt` can resolve the key without a `KeyId` parameter.
- Keys are created in `Enabled` state immediately — no provisioning delay.
- `ScheduleKeyDeletion` sets state to `PendingDeletion` but does **not** actually delete the key after the window in AWSim.
- `GenerateDataKey` returns both a plaintext key (for immediate use) and an encrypted copy (to store) — this enables envelope encryption without exposing the master key.
- `GenerateRandom` returns cryptographically random bytes and does not require a key ID.
- Key rotation (`EnableKeyRotation`, `DisableKeyRotation`) state is stored per key but no actual rotation occurs in AWSim.
- Grants (`CreateGrant`, `ListGrants`, etc.) are stored and returned correctly but are not enforced for cryptographic operations.
- Key policies (`GetKeyPolicy`, `PutKeyPolicy`) are stored and returned but are not enforced.
- Key material is in-memory only and lost on restart (no persistence).
