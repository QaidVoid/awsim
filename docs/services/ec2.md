# EC2

Amazon EC2 networking primitives — VPCs, subnets, security groups, route tables, internet gateways, and key pairs.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsQuery` |
| Signing Name | `ec2` |
| Persistence | No |

## Operations

### VPCs
- `CreateVpc` — create a Virtual Private Cloud with a CIDR block
- `DeleteVpc` — delete a VPC
- `DescribeVpcs` — list VPCs with optional filters

### Subnets
- `CreateSubnet` — create a subnet within a VPC
- `DeleteSubnet` — delete a subnet
- `DescribeSubnets` — list subnets with optional filters

### Security Groups
- `CreateSecurityGroup` — create a security group within a VPC
- `DeleteSecurityGroup` — delete a security group
- `DescribeSecurityGroups` — list security groups with optional filters
- `AuthorizeSecurityGroupIngress` — add inbound rules to a security group
- `AuthorizeSecurityGroupEgress` — add outbound rules to a security group
- `RevokeSecurityGroupIngress` — remove inbound rules from a security group
- `RevokeSecurityGroupEgress` — remove outbound rules from a security group

### Internet Gateways
- `CreateInternetGateway` — create an internet gateway
- `DeleteInternetGateway` — delete an internet gateway
- `AttachInternetGateway` — attach an internet gateway to a VPC
- `DetachInternetGateway` — detach an internet gateway from a VPC
- `DescribeInternetGateways` — list internet gateways with optional filters

### Route Tables
- `CreateRouteTable` — create a route table in a VPC
- `DeleteRouteTable` — delete a route table
- `DescribeRouteTables` — list route tables with optional filters
- `CreateRoute` — add a route to a route table
- `AssociateRouteTable` — associate a route table with a subnet

### Key Pairs
- `CreateKeyPair` — create an EC2 key pair for SSH access
- `DeleteKeyPair` — delete a key pair
- `DescribeKeyPairs` — list key pairs

### Metadata
- `DescribeRegions` — list available AWS regions
- `DescribeAvailabilityZones` — list availability zones in the current region

## Example

```bash
# Create a VPC
aws --endpoint-url http://localhost:4567 \
  ec2 create-vpc \
  --cidr-block 10.0.0.0/16

# Create a subnet
aws --endpoint-url http://localhost:4567 \
  ec2 create-subnet \
  --vpc-id <vpc-id> \
  --cidr-block 10.0.1.0/24 \
  --availability-zone us-east-1a

# Create a security group
aws --endpoint-url http://localhost:4567 \
  ec2 create-security-group \
  --group-name web-sg \
  --description "Web server security group" \
  --vpc-id <vpc-id>

# Allow HTTP inbound
aws --endpoint-url http://localhost:4567 \
  ec2 authorize-security-group-ingress \
  --group-id <sg-id> \
  --protocol tcp \
  --port 80 \
  --cidr 0.0.0.0/0
```

## Notes

- AWSim implements EC2 networking primitives only. Compute resources (instances, AMIs, EBS volumes, Auto Scaling) are not supported.
- EC2 uses the `AwsQuery` protocol (form-encoded POST requests with `Action=` parameter).
- Resource IDs are generated with the standard `vpc-`, `subnet-`, `sg-`, `igw-`, `rtb-`, `keypair-` prefixes.
- State is in-memory only and lost on restart.
