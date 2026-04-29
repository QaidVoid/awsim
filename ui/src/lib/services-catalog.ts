/**
 * Service catalog — the canonical list of every AWS service the UI knows
 * about, grouped by category for the sidebar and command palette.
 *
 * Adding a new service? Drop it in here and the sidebar + Cmd-K palette
 * pick it up automatically.
 */

import Activity from "@lucide/svelte/icons/activity";
import Archive from "@lucide/svelte/icons/archive";
import BarChart from "@lucide/svelte/icons/bar-chart-3";
import BellRing from "@lucide/svelte/icons/bell-ring";
import Boxes from "@lucide/svelte/icons/boxes";
import Brain from "@lucide/svelte/icons/brain";
import Building2 from "@lucide/svelte/icons/building-2";
import Cable from "@lucide/svelte/icons/cable";
import Calendar from "@lucide/svelte/icons/calendar";
import Cloud from "@lucide/svelte/icons/cloud";
import CloudCog from "@lucide/svelte/icons/cloud-cog";
import Container from "@lucide/svelte/icons/container";
import Cpu from "@lucide/svelte/icons/cpu";
import Database from "@lucide/svelte/icons/database";
import DollarSign from "@lucide/svelte/icons/dollar-sign";
import FileSearch from "@lucide/svelte/icons/file-search";
import Fingerprint from "@lucide/svelte/icons/fingerprint";
import Flame from "@lucide/svelte/icons/flame";
import Gauge from "@lucide/svelte/icons/gauge";
import Globe from "@lucide/svelte/icons/globe";
import BookText from "@lucide/svelte/icons/book-text";
import HardDrive from "@lucide/svelte/icons/hard-drive";
import KeyRound from "@lucide/svelte/icons/key-round";
import Layers from "@lucide/svelte/icons/layers";
import MemoryStick from "@lucide/svelte/icons/memory-stick";
import Lock from "@lucide/svelte/icons/lock";
import MapPin from "@lucide/svelte/icons/map-pin";
import Megaphone from "@lucide/svelte/icons/megaphone";
import Mail from "@lucide/svelte/icons/mail";
import MessageSquare from "@lucide/svelte/icons/message-square";
import MessagesSquare from "@lucide/svelte/icons/messages-square";
import MicVocal from "@lucide/svelte/icons/mic-vocal";
import Network from "@lucide/svelte/icons/network";
import Package from "@lucide/svelte/icons/package";
import Radar from "@lucide/svelte/icons/radar";
import Route from "@lucide/svelte/icons/route";
import Scale from "@lucide/svelte/icons/scale";
import ScrollText from "@lucide/svelte/icons/scroll-text";
import Search from "@lucide/svelte/icons/search";
import Server from "@lucide/svelte/icons/server";
import Settings from "@lucide/svelte/icons/settings";
import Share2 from "@lucide/svelte/icons/share-2";
import Shield from "@lucide/svelte/icons/shield";
import ShieldCheck from "@lucide/svelte/icons/shield-check";
import Snowflake from "@lucide/svelte/icons/snowflake";
import SquareTerminal from "@lucide/svelte/icons/square-terminal";
import Tag from "@lucide/svelte/icons/tag";
import Upload from "@lucide/svelte/icons/upload";
import UsersRound from "@lucide/svelte/icons/users-round";
import ToggleLeft from "@lucide/svelte/icons/toggle-left";
import Workflow from "@lucide/svelte/icons/workflow";
import Zap from "@lucide/svelte/icons/zap";
import type { Component } from "svelte";

export type ServiceCategory =
  | "Compute"
  | "Storage"
  | "Messaging"
  | "Observability"
  | "Security"
  | "AI/ML"
  | "Networking"
  | "Workflows"
  | "Data"
  | "Admin";

export interface Service {
  id: string;
  name: string;
  href: string;
  category: ServiceCategory;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  icon: Component<any>;
  keywords?: string[];
}

export const CATEGORY_ORDER: ServiceCategory[] = [
  "Compute",
  "Storage",
  "Messaging",
  "Observability",
  "Security",
  "AI/ML",
  "Networking",
  "Workflows",
  "Data",
  "Admin",
];

export const SERVICES: Service[] = [
  // Compute
  {
    id: "lambda",
    name: "Lambda",
    href: "/lambda",
    category: "Compute",
    icon: Zap,
    keywords: ["function", "serverless"],
  },
  {
    id: "ec2",
    name: "EC2",
    href: "/ec2",
    category: "Compute",
    icon: Server,
    keywords: ["vm", "instance"],
  },
  {
    id: "ecs",
    name: "ECS",
    href: "/ecs",
    category: "Compute",
    icon: Container,
    keywords: ["container", "fargate"],
  },
  {
    id: "eks",
    name: "EKS",
    href: "/eks",
    category: "Compute",
    icon: Boxes,
    keywords: ["kubernetes", "k8s"],
  },
  {
    id: "batch",
    name: "Batch",
    href: "/batch",
    category: "Compute",
    icon: Cpu,
    keywords: ["jobs"],
  },

  // Storage
  {
    id: "s3",
    name: "S3",
    href: "/s3",
    category: "Storage",
    icon: Package,
    keywords: ["bucket", "object"],
  },
  {
    id: "dynamodb",
    name: "DynamoDB",
    href: "/dynamodb",
    category: "Storage",
    icon: Database,
    keywords: ["nosql", "table"],
  },
  {
    id: "rds",
    name: "RDS",
    href: "/rds",
    category: "Storage",
    icon: HardDrive,
    keywords: ["sql", "postgres", "mysql"],
  },
  {
    id: "memorydb",
    name: "MemoryDB",
    href: "/memorydb",
    category: "Storage",
    icon: MemoryStick,
    keywords: ["memorydb", "redis", "in-memory", "cache"],
  },
  {
    id: "qldb",
    name: "QLDB",
    href: "/qldb",
    category: "Storage",
    icon: BookText,
    keywords: ["ledger", "qldb", "quantum ledger"],
  },
  {
    id: "transfer",
    name: "Transfer Family",
    href: "/transfer",
    category: "Storage",
    icon: Upload,
    keywords: ["sftp", "ftp", "ftps", "transfer family"],
  },
  {
    id: "efs",
    name: "EFS",
    href: "/efs",
    category: "Storage",
    icon: HardDrive,
    keywords: ["file system", "nfs", "elastic file"],
  },
  {
    id: "backup",
    name: "Backup",
    href: "/backup",
    category: "Storage",
    icon: ShieldCheck,
    keywords: ["backup", "vault", "recovery point", "snapshot"],
  },
  {
    id: "glacier",
    name: "Glacier",
    href: "/glacier",
    category: "Storage",
    icon: Snowflake,
    keywords: ["glacier", "cold storage", "archive", "vault"],
  },

  // Messaging
  {
    id: "sqs",
    name: "SQS",
    href: "/sqs",
    category: "Messaging",
    icon: MessageSquare,
    keywords: ["queue"],
  },
  {
    id: "sns",
    name: "SNS",
    href: "/sns",
    category: "Messaging",
    icon: BellRing,
    keywords: ["topic", "pubsub"],
  },
  {
    id: "mq",
    name: "MQ",
    href: "/mq",
    category: "Messaging",
    icon: MessagesSquare,
    keywords: ["amazon mq", "rabbitmq", "activemq", "broker"],
  },
  {
    id: "eventbridge",
    name: "EventBridge",
    href: "/eventbridge",
    category: "Messaging",
    icon: Share2,
    keywords: ["events", "bus"],
  },
  {
    id: "firehose",
    name: "Firehose",
    href: "/firehose",
    category: "Messaging",
    icon: Flame,
    keywords: ["stream", "delivery"],
  },
  {
    id: "kinesis",
    name: "Kinesis",
    href: "/kinesis",
    category: "Messaging",
    icon: Activity,
    keywords: ["stream"],
  },
  {
    id: "ses",
    name: "SES",
    href: "/ses",
    category: "Messaging",
    icon: Mail,
    keywords: ["email"],
  },
  {
    id: "pinpoint",
    name: "Pinpoint",
    href: "/pinpoint",
    category: "Messaging",
    icon: Megaphone,
    keywords: ["pinpoint", "campaign", "endpoint", "segment", "marketing"],
  },
  {
    id: "pipes",
    name: "EventBridge Pipes",
    href: "/pipes",
    category: "Messaging",
    icon: Cable,
    keywords: ["pipe", "source", "target", "filter", "enrichment"],
  },

  // Observability
  {
    id: "logs",
    name: "CloudWatch Logs",
    href: "/cloudwatch",
    category: "Observability",
    icon: ScrollText,
    keywords: ["logs"],
  },
  {
    id: "metrics",
    name: "CloudWatch Metrics",
    href: "/monitoring",
    category: "Observability",
    icon: BarChart,
    keywords: ["metrics", "monitoring"],
  },
  {
    id: "xray",
    name: "X-Ray",
    href: "/xray",
    category: "Observability",
    icon: Activity,
    keywords: ["trace", "tracing", "service graph", "segments"],
  },
  {
    id: "cloudtrail",
    name: "CloudTrail",
    href: "/cloudtrail",
    category: "Observability",
    icon: Radar,
    keywords: ["audit", "trail"],
  },
  {
    id: "request-log",
    name: "Request Log",
    href: "/logs",
    category: "Observability",
    icon: FileSearch,
    keywords: ["requests", "http"],
  },

  // Security
  {
    id: "iam",
    name: "IAM",
    href: "/iam",
    category: "Security",
    icon: KeyRound,
    keywords: ["user", "role", "policy"],
  },
  {
    id: "cognito",
    name: "Cognito",
    href: "/cognito",
    category: "Security",
    icon: Fingerprint,
    keywords: ["auth", "user pool"],
  },
  {
    id: "kms",
    name: "KMS",
    href: "/kms",
    category: "Security",
    icon: Lock,
    keywords: ["key", "encryption"],
  },
  {
    id: "acm",
    name: "ACM",
    href: "/acm",
    category: "Security",
    icon: ShieldCheck,
    keywords: ["certificate", "tls"],
  },
  {
    id: "secrets",
    name: "Secrets Manager",
    href: "/secrets",
    category: "Security",
    icon: KeyRound,
    keywords: ["secret"],
  },
  {
    id: "waf",
    name: "WAF",
    href: "/waf",
    category: "Security",
    icon: Shield,
    keywords: ["firewall"],
  },
  {
    id: "sts",
    name: "STS",
    href: "/sts",
    category: "Security",
    icon: Tag,
    keywords: ["token", "assume role"],
  },

  // AI/ML
  {
    id: "bedrock",
    name: "Bedrock",
    href: "/bedrock",
    category: "AI/ML",
    icon: Brain,
    keywords: ["llm", "foundation model"],
  },
  {
    id: "polly",
    name: "Polly",
    href: "/polly",
    category: "AI/ML",
    icon: MicVocal,
    keywords: ["tts", "speech"],
  },

  // Networking
  {
    id: "route53",
    name: "Route 53",
    href: "/route53",
    category: "Networking",
    icon: Route,
    keywords: ["dns", "domain"],
  },
  {
    id: "elb",
    name: "ELB",
    href: "/elb",
    category: "Networking",
    icon: Scale,
    keywords: ["load balancer"],
  },
  {
    id: "cloudfront",
    name: "CloudFront",
    href: "/cloudfront",
    category: "Networking",
    icon: Globe,
    keywords: ["cdn"],
  },
  {
    id: "apigateway",
    name: "API Gateway",
    href: "/apigateway",
    category: "Networking",
    icon: Network,
    keywords: ["rest", "http api"],
  },
  {
    id: "appsync",
    name: "AppSync",
    href: "/appsync",
    category: "Networking",
    icon: Cable,
    keywords: ["graphql"],
  },
  {
    id: "servicediscovery",
    name: "Cloud Map",
    href: "/servicediscovery",
    category: "Networking",
    icon: MapPin,
    keywords: ["service discovery", "cloud map", "namespace", "instance"],
  },

  // Workflows
  {
    id: "stepfunctions",
    name: "Step Functions",
    href: "/stepfunctions",
    category: "Workflows",
    icon: Workflow,
    keywords: ["state machine", "sfn"],
  },
  {
    id: "application-autoscaling",
    name: "Application Auto Scaling",
    href: "/application-autoscaling",
    category: "Workflows",
    icon: Gauge,
    keywords: ["autoscaling", "scaling policy", "ecs", "lambda", "dynamodb"],
  },
  {
    id: "appconfig",
    name: "AppConfig",
    href: "/appconfig",
    category: "Workflows",
    icon: ToggleLeft,
    keywords: ["feature flag", "appconfig", "config delivery", "deployment"],
  },
  {
    id: "scheduler",
    name: "Scheduler",
    href: "/scheduler",
    category: "Workflows",
    icon: Calendar,
    keywords: ["cron"],
  },

  // Data
  {
    id: "athena",
    name: "Athena",
    href: "/athena",
    category: "Data",
    icon: Search,
    keywords: ["query", "sql"],
  },
  {
    id: "glue",
    name: "Glue",
    href: "/glue",
    category: "Data",
    icon: Layers,
    keywords: ["etl", "catalog"],
  },
  {
    id: "datasync",
    name: "DataSync",
    href: "/datasync",
    category: "Data",
    icon: Cloud,
    keywords: ["transfer"],
  },

  // Admin
  {
    id: "organizations",
    name: "Organizations",
    href: "/organizations",
    category: "Admin",
    icon: Building2,
    keywords: ["org", "account"],
  },
  {
    id: "sso",
    name: "IAM Identity Center",
    href: "/sso",
    category: "Admin",
    icon: KeyRound,
    keywords: ["sso"],
  },
  {
    id: "identitystore",
    name: "Identity Store",
    href: "/identitystore",
    category: "Admin",
    icon: UsersRound,
    keywords: ["identity store", "users", "groups", "sso directory"],
  },
  {
    id: "ssm",
    name: "SSM",
    href: "/ssm",
    category: "Admin",
    icon: Settings,
    keywords: ["parameter", "systems manager"],
  },
  {
    id: "ecr",
    name: "ECR",
    href: "/ecr",
    category: "Admin",
    icon: Archive,
    keywords: ["registry", "container image"],
  },
  {
    id: "cloudformation",
    name: "CloudFormation",
    href: "/cloudformation",
    category: "Admin",
    icon: CloudCog,
    keywords: ["stack", "iac"],
  },
  {
    id: "resourcegroupstagging",
    name: "Resource Tags",
    href: "/resourcegroupstagging",
    category: "Admin",
    icon: Tag,
    keywords: ["tagging", "tag", "resource groups", "discovery"],
  },
  {
    id: "billing",
    name: "Billing",
    href: "/billing",
    category: "Admin",
    icon: DollarSign,
    keywords: ["pricing", "cost", "bill", "spend", "estimate"],
  },
  {
    id: "chaos",
    name: "Chaos",
    href: "/chaos",
    category: "Admin",
    icon: Flame,
    keywords: ["chaos", "fault", "inject", "failure", "latency", "throttle"],
  },
  {
    id: "playground",
    name: "Playground",
    href: "/playground",
    category: "Admin",
    icon: SquareTerminal,
    keywords: ["playground", "request", "builder", "test", "rest", "json"],
  },
];

export function servicesByCategory(): Map<ServiceCategory, Service[]> {
  const grouped = new Map<ServiceCategory, Service[]>();
  for (const cat of CATEGORY_ORDER) {
    grouped.set(cat, []);
  }
  for (const svc of SERVICES) {
    grouped.get(svc.category)!.push(svc);
  }
  for (const cat of CATEGORY_ORDER) {
    grouped.get(cat)!.sort((a, b) => a.name.localeCompare(b.name));
  }
  return grouped;
}

export function findService(id: string): Service | undefined {
  return SERVICES.find((s) => s.id === id);
}

export function findServiceByPath(path: string): Service | undefined {
  if (!path || path === "/") return undefined;
  // Match the longest service href prefix (so /s3/buckets/xyz still maps to S3).
  const match = SERVICES.filter(
    (s) => path === s.href || path.startsWith(s.href + "/"),
  ).sort((a, b) => b.href.length - a.href.length)[0];
  return match;
}

// Re-export icon utility used by SquareTerminal references in components.
export { SquareTerminal };
