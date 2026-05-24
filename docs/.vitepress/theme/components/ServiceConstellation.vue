<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'

// keep this list and the heat ordering loosely matched to docs/services/*.
// "heat" influences pulse delay so the grid breathes instead of strobing.
type Svc = { id: string; label: string; tier: 1 | 2 | 3 }
const services: Svc[] = [
  { id: 's3', label: 'S3', tier: 1 },
  { id: 'dynamodb', label: 'DynamoDB', tier: 1 },
  { id: 'lambda', label: 'Lambda', tier: 1 },
  { id: 'sqs', label: 'SQS', tier: 1 },
  { id: 'sns', label: 'SNS', tier: 1 },
  { id: 'iam', label: 'IAM', tier: 1 },
  { id: 'cognito', label: 'Cognito', tier: 1 },
  { id: 'apigateway', label: 'API Gateway', tier: 1 },
  { id: 'kms', label: 'KMS', tier: 1 },
  { id: 'secretsmanager', label: 'Secrets', tier: 1 },
  { id: 'ssm', label: 'SSM', tier: 1 },
  { id: 'cloudwatch-logs', label: 'CW Logs', tier: 1 },
  { id: 'cloudwatch-metrics', label: 'CW Metrics', tier: 1 },
  { id: 'eventbridge', label: 'EventBridge', tier: 2 },
  { id: 'scheduler', label: 'Scheduler', tier: 2 },
  { id: 'pipes', label: 'Pipes', tier: 2 },
  { id: 'stepfunctions', label: 'Step Fn', tier: 2 },
  { id: 'kinesis', label: 'Kinesis', tier: 2 },
  { id: 'ses', label: 'SES', tier: 2 },
  { id: 'opensearch', label: 'OpenSearch', tier: 2 },
  { id: 'bedrock', label: 'Bedrock', tier: 2 },
  { id: 'kendra', label: 'Kendra', tier: 2 },
  { id: 'comprehend', label: 'Comprehend', tier: 2 },
  { id: 'athena', label: 'Athena', tier: 2 },
  { id: 'glue', label: 'Glue', tier: 2 },
  { id: 'ec2', label: 'EC2', tier: 2 },
  { id: 'ecs', label: 'ECS', tier: 2 },
  { id: 'ecr', label: 'ECR', tier: 2 },
  { id: 'elb', label: 'ELB', tier: 2 },
  { id: 'rds', label: 'RDS', tier: 2 },
  { id: 'cloudfront', label: 'CloudFront', tier: 2 },
  { id: 'route53', label: 'Route 53', tier: 2 },
  { id: 'cloudformation', label: 'CloudFmt', tier: 2 },
  { id: 'appsync', label: 'AppSync', tier: 2 },
  { id: 'acm', label: 'ACM', tier: 3 },
  { id: 'waf', label: 'WAF', tier: 3 },
  { id: 'identitystore', label: 'IdStore', tier: 3 },
  { id: 'servicediscovery', label: 'Svc Disco', tier: 3 },
  { id: 'resourcegroupstagging', label: 'Tag Groups', tier: 3 },
  { id: 'application-autoscaling', label: 'AutoScale', tier: 3 },
  { id: 'appconfig', label: 'AppConfig', tier: 3 },
  { id: 'backup', label: 'Backup', tier: 3 },
  { id: 'transfer', label: 'Transfer', tier: 3 },
  { id: 'mq', label: 'MQ', tier: 3 },
  { id: 'memorydb', label: 'MemoryDB', tier: 3 },
  { id: 'neptune', label: 'Neptune', tier: 3 },
  { id: 'docdb', label: 'DocumentDB', tier: 3 },
  { id: 'qldb', label: 'QLDB', tier: 3 },
  { id: 'efs', label: 'EFS', tier: 3 },
  { id: 'glacier', label: 'Glacier', tier: 3 },
  { id: 'xray', label: 'X-Ray', tier: 3 },
  { id: 'pinpoint', label: 'Pinpoint', tier: 3 },
]

const focused = ref<string | null>(null)
const heat = ref<number[]>([])

let timer: ReturnType<typeof setInterval> | null = null

onMounted(() => {
  heat.value = services.map(() => Math.random())
  // sparkly periodic re-roll so a few tiles "fire" at any moment
  timer = setInterval(() => {
    const i = Math.floor(Math.random() * services.length)
    heat.value[i] = Math.random()
  }, 220)
})

onUnmounted(() => { if (timer) clearInterval(timer) })
</script>

<template>
  <section class="constellation">
    <header class="head">
      <div class="title">
        <span class="prefix">/_awsim/</span><span class="word">services</span>
        <span class="count">{{ services.length }} online</span>
      </div>
      <div class="hint">click a tile -> per-service docs</div>
    </header>

    <div class="grid">
      <a
        v-for="(s, i) in services"
        :key="s.id"
        :href="`/services/${s.id}.html`"
        :class="['tile', 'tier-' + s.tier, focused === s.id ? 'focus' : '']"
        @mouseenter="focused = s.id"
        @mouseleave="focused = null"
      >
        <span class="dot" :style="{ animationDelay: (heat[i] || 0) * 2 + 's' }"></span>
        <span class="label">{{ s.label }}</span>
        <span class="id">{{ s.id }}</span>
      </a>
    </div>
  </section>
</template>

<style scoped>
.constellation {
  border: 1px solid var(--awsim-edge);
  border-radius: 10px;
  background: var(--awsim-panel);
  padding: 18px;
  font-family: var(--awsim-mono);
}
.head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  margin-bottom: 14px;
}
.title {
  font-size: 13px;
  color: #d6c8a6;
}
.prefix { color: #7a7468; }
.word { color: var(--awsim-orange); }
.count {
  margin-left: 12px;
  color: #6dcf6a;
  font-size: 11px;
  letter-spacing: 0.05em;
}
.hint { color: #6c6453; font-size: 11px; }

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(118px, 1fr));
  gap: 6px;
}

.tile {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 10px 10px 9px;
  background: #0e0d0b;
  border: 1px solid #1c1a17;
  border-radius: 6px;
  text-decoration: none;
  color: #d6c8a6;
  transition: all 120ms ease;
  overflow: hidden;
}
.tile:hover, .tile.focus {
  border-color: var(--awsim-orange);
  background: #181410;
  transform: translateY(-1px);
  box-shadow: 0 6px 20px -10px rgba(255,153,0,0.6);
}
.tile .label {
  font-size: 12.5px;
  font-weight: 500;
  color: #ecdfc0;
}
.tile .id {
  font-size: 10px;
  color: #6c6453;
  letter-spacing: 0.03em;
}
.tile .dot {
  position: absolute;
  top: 8px; right: 8px;
  width: 6px; height: 6px; border-radius: 50%;
  background: #2a4d2a;
  animation: heat 4.5s ease-in-out infinite;
}
.tier-1 .dot { background: #5cc26b; }
.tier-2 .dot { background: #c2a55c; }
.tier-3 .dot { background: #5c8ac2; }

@keyframes heat {
  0%, 80%, 100% { box-shadow: 0 0 0 0 rgba(108,207,106,0); transform: scale(1); }
  88%           { box-shadow: 0 0 8px 2px rgba(108,207,106,0.7); transform: scale(1.5); }
}
</style>
