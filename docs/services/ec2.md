# EC2

Amazon EC2 networking primitives — VPCs, subnets, security groups, route tables, internet gateways, and key pairs.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsQuery` |
| Signing Name | `ec2` |
| Persistence | No |

EC2 uses the `AwsQuery` protocol: `POST` requests with `Content-Type: application/x-www-form-urlencoded` and an `Action=` parameter.

> **Note:** AWSim implements EC2 networking primitives only. Compute resources (instances, AMIs, EBS volumes, Auto Scaling) are **not** supported.

## Quick Start

Create a VPC, add a subnet, and set up a security group with an inbound HTTP rule:

```bash
# Create a VPC
VPC_ID=$(curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ec2/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=CreateVpc' \
  --data-urlencode 'CidrBlock=10.0.0.0/16' \
  | grep -o '<vpcId>[^<]*' | sed 's/<vpcId>//')

# Create a subnet
SUBNET_ID=$(curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ec2/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=CreateSubnet' \
  --data-urlencode "VpcId=$VPC_ID" \
  --data-urlencode 'CidrBlock=10.0.1.0/24' \
  --data-urlencode 'AvailabilityZone=us-east-1a' \
  | grep -o '<subnetId>[^<]*' | sed 's/<subnetId>//')

echo "VPC: $VPC_ID, Subnet: $SUBNET_ID"
```

Using the AWS CLI is recommended for EC2's XML-heavy responses:

```bash
# Create VPC
aws --endpoint-url http://localhost:4566 ec2 create-vpc --cidr-block 10.0.0.0/16

# Create subnet
aws --endpoint-url http://localhost:4566 ec2 create-subnet \
  --vpc-id vpc-REPLACE_ME --cidr-block 10.0.1.0/24 --availability-zone us-east-1a

# Create security group
aws --endpoint-url http://localhost:4566 ec2 create-security-group \
  --group-name web-sg --description "Web server SG" --vpc-id vpc-REPLACE_ME

# Allow HTTP inbound
aws --endpoint-url http://localhost:4566 ec2 authorize-security-group-ingress \
  --group-id sg-REPLACE_ME --protocol tcp --port 80 --cidr 0.0.0.0/0
```

## Operations

### VPCs
- `CreateVpc` — create a Virtual Private Cloud with a CIDR block
  - Input: `CidrBlock` (required, e.g., `10.0.0.0/16`), optional `TagSpecification`
  - Returns: `vpc` element with `vpcId` (e.g., `vpc-abc12345`), `cidrBlock`, `state` (`available`), `dhcpOptionsId`

- `DeleteVpc` — delete a VPC (must have no subnets or security groups)
  - Input: `VpcId`

- `DescribeVpcs` — list VPCs with optional filters
  - Input: optional `VpcId.N` (list), `Filter.N` (name-values)
  - Returns: `vpcSet` with matching VPCs

### Subnets
- `CreateSubnet` — create a subnet within a VPC
  - Input: `VpcId`, `CidrBlock` (must be within VPC CIDR, e.g., `10.0.1.0/24`), optional `AvailabilityZone`
  - Returns: `subnet` with `subnetId`, `availabilityZone`, `availableIpAddressCount`

- `DeleteSubnet` — delete a subnet
- `DescribeSubnets` — list subnets with optional filters (`vpc-id`, `subnet-id`, etc.)

### Security Groups
- `CreateSecurityGroup` — create a security group within a VPC
  - Input: `GroupName`, `Description`, `VpcId`
  - Returns: `groupId` (e.g., `sg-abc12345`)

- `DeleteSecurityGroup` — delete a security group
- `DescribeSecurityGroups` — list security groups with optional filters

- `AuthorizeSecurityGroupIngress` — add inbound rules to a security group
  - Input: `GroupId`, `IpPermissions` (list with `IpProtocol`, `FromPort`, `ToPort`, `IpRanges`)
  - Shorthand: `--protocol tcp --port 443 --cidr 0.0.0.0/0`

- `AuthorizeSecurityGroupEgress` — add outbound rules
- `RevokeSecurityGroupIngress` — remove inbound rules
- `RevokeSecurityGroupEgress` — remove outbound rules

### Internet Gateways
- `CreateInternetGateway` — create an internet gateway
  - Returns: `internetGateway` with `internetGatewayId` (e.g., `igw-abc12345`), `attachmentSet` (empty until attached)

- `AttachInternetGateway` — attach an internet gateway to a VPC
  - Input: `InternetGatewayId`, `VpcId`

- `DetachInternetGateway` — detach from a VPC (must happen before deletion)
- `DeleteInternetGateway` — delete an internet gateway
- `DescribeInternetGateways` — list internet gateways with filters

### Route Tables
- `CreateRouteTable` — create a route table in a VPC
  - Input: `VpcId`
  - Returns: `routeTable` with `routeTableId`, default local route already included

- `CreateRoute` — add a route to a route table
  - Input: `RouteTableId`, `DestinationCidrBlock`, `GatewayId` (e.g., an internet gateway ID)

- `AssociateRouteTable` — associate a route table with a subnet
  - Input: `RouteTableId`, `SubnetId`
  - Returns: `associationId`

- `DeleteRouteTable` — delete a route table (must be disassociated first)
- `DescribeRouteTables` — list route tables with filters

### Key Pairs
- `CreateKeyPair` — create an EC2 key pair for SSH access
  - Input: `KeyName`
  - Returns: `keyName`, `keyFingerprint`, `keyMaterial` (PEM private key — only returned on creation)

- `DeleteKeyPair` — delete a key pair (does not affect existing instances)
- `DescribeKeyPairs` — list key pairs (private key material is not returned)

### Metadata
- `DescribeRegions` — list available AWS regions
  - Returns a list of region names (e.g., `us-east-1`, `eu-west-1`)

- `DescribeAvailabilityZones` — list availability zones in the current region
  - Returns zones like `us-east-1a`, `us-east-1b`, `us-east-1c`

### Tags (EC2 Tag API)
- `CreateTags` — add or overwrite tags on any EC2 resource
  - Input: `ResourceId.N` (list of resource IDs), `Tag.N.Key` / `Tag.N.Value`
  - Applies to VPCs, subnets, security groups, internet gateways, route tables, instances

- `DeleteTags` — remove tags from resources
  - Input: `ResourceId.N`, `Tag.N.Key` (value is optional — only key is matched)

- `DescribeTags` — list all tags across all tagged resources
  - Returns: `tagSet` list with `key`, `value`, `resourceId`, `resourceType`

### Instances

EC2 instances are modelled as a state machine — no real VMs run, but the lifecycle (`pending` / `running` / `stopping` / `stopped` / `shutting-down` / `terminated`) is honored end-to-end with proper numeric state codes.

- `RunInstances` — register one or more instances under a single reservation
  - Input: `ImageId` (default `ami-00000000`), `InstanceType` (default `t2.micro`), `MinCount`, `MaxCount`, optional `SubnetId`
  - Returns `instancesSet` (with `instanceId`, `instanceType`, `imageId`, `instanceState`, `privateIpAddress`, `launchTime`, `reservationId`) and a single shared `reservationId`
  - Private IPs are allocated from the launch subnet's CIDR via a per-subnet host counter starting at `.10`. With no `SubnetId`, falls back to `10.0.0.10`
  - Instances land in `running` immediately

- `DescribeInstances` — list instances grouped by reservation
  - Filters: `InstanceId.N`, plus `Filter.N.{Name=instance-state-name, Value.N}` for state-based filtering (the one `aws ec2 wait` reaches for)

- `StartInstances` — `stopped` → `running`. Out-of-order calls (e.g. on a `running` instance) are well-shaped no-ops

- `StopInstances` — `running` → `stopped`. Same no-op semantics for invalid predecessors

- `RebootInstances` — fire-and-forget; instance stays in `running`

- `TerminateInstances` — anything-not-already-terminated → `terminated`. Returns the proper `currentState`/`previousState` shape

- `DescribeInstanceStatus` — returns the lifecycle state for non-terminated instances. Honors `IncludeAllInstances` to surface stopped/terminated entries too

- `DescribeImages` — returns a small built-in AMI catalog (Amazon Linux 2, Ubuntu 22.04)

### Network / VPC Stubs
- `DescribeNetworkInterfaces` — returns empty `networkInterfaceSet`
- `DescribeNatGateways` — returns empty `natGatewaySet`
- `DescribeVpcEndpoints` — returns empty `vpcEndpointSet`
- `DescribeAddresses` — list Elastic IPs (returns stored addresses, empty by default)

## Curl Examples

```bash
# 1. Create a security group
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ec2/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=CreateSecurityGroup' \
  --data-urlencode 'GroupName=app-sg' \
  --data-urlencode 'Description=Application security group' \
  --data-urlencode 'VpcId=vpc-YOUR_ID'

# 2. Authorize HTTPS inbound
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ec2/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=AuthorizeSecurityGroupIngress' \
  --data-urlencode 'GroupId=sg-YOUR_ID' \
  --data-urlencode 'IpPermissions.1.IpProtocol=tcp' \
  --data-urlencode 'IpPermissions.1.FromPort=443' \
  --data-urlencode 'IpPermissions.1.ToPort=443' \
  --data-urlencode 'IpPermissions.1.IpRanges.1.CidrIp=0.0.0.0/0'

# 3. Describe regions
curl -s -X POST http://localhost:4566 \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/ec2/aws4_request, SignedHeaders=host, Signature=fake" \
  --data-urlencode 'Action=DescribeRegions'
```

## SDK Example

```typescript
import {
  EC2Client,
  CreateVpcCommand,
  CreateSubnetCommand,
  CreateSecurityGroupCommand,
  AuthorizeSecurityGroupIngressCommand,
} from '@aws-sdk/client-ec2';

const ec2 = new EC2Client({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

// Create VPC
const { Vpc } = await ec2.send(new CreateVpcCommand({
  CidrBlock: '10.0.0.0/16',
}));
const vpcId = Vpc!.VpcId!;

// Create subnet
const { Subnet } = await ec2.send(new CreateSubnetCommand({
  VpcId: vpcId,
  CidrBlock: '10.0.1.0/24',
  AvailabilityZone: 'us-east-1a',
}));

// Create security group
const { GroupId } = await ec2.send(new CreateSecurityGroupCommand({
  GroupName: 'web-sg',
  Description: 'Web server security group',
  VpcId: vpcId,
}));

// Allow HTTP and HTTPS inbound
await ec2.send(new AuthorizeSecurityGroupIngressCommand({
  GroupId,
  IpPermissions: [
    { IpProtocol: 'tcp', FromPort: 80, ToPort: 80, IpRanges: [{ CidrIp: '0.0.0.0/0' }] },
    { IpProtocol: 'tcp', FromPort: 443, ToPort: 443, IpRanges: [{ CidrIp: '0.0.0.0/0' }] },
  ],
}));

console.log('VPC:', vpcId, '| Subnet:', Subnet?.SubnetId, '| SG:', GroupId);
```

## Behavior Notes

- `RunInstances` creates in-memory instance records; no compute is allocated and the instance never actually runs.
- The lifecycle state machine is real: `Stop` only works from `running`, `Start` only from `stopped`, etc. Out-of-order transitions are no-ops. `Terminated` instances stay queryable for the lifetime of the process so post-terminate `DescribeInstances` calls still see them (real EC2 also retains records ~1 hour).
- Tags created with `CreateTags` are reflected on the underlying resource object (e.g., a tagged VPC shows tags in `DescribeVpcs`).
- `DescribeNatGateways`, `DescribeVpcEndpoints`, `DescribeNetworkInterfaces` always return empty result sets. `DescribeImages` returns a small built-in catalog so listings aren't empty.
- No userdata execution, no EBS volumes, no IMDS endpoint at 169.254.169.254 — see [the EC2 explainer in CONTRIBUTING](#) for what's modelled vs stubbed.
- Resource IDs are generated with standard prefixes: `vpc-`, `subnet-`, `sg-`, `igw-`, `rtb-`, `keypair-`, `i-`.
- `DescribeRegions` returns a hardcoded list of AWS regions (same as real AWS, not dynamic).
- Security group rules are stored but not enforced — no actual network traffic filtering occurs.
- State is in-memory only and lost on restart.
