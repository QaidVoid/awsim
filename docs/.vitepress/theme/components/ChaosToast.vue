<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'

// Chaos engine joke. Picks a random "fault" every ~14s and surfaces it as a
// dismissable toast so the page itself feels like it's running through awsim.
const events = [
  { svc: 's3',       op: 'PutObject',    fault: 'SlowDown',          ms: 1820 },
  { svc: 'dynamodb', op: 'PutItem',      fault: 'ProvisionedThroughputExceeded', ms: 12 },
  { svc: 'lambda',   op: 'Invoke',       fault: 'TooManyRequests',   ms: 3 },
  { svc: 'sqs',      op: 'ReceiveMessage', fault: 'latency-injected', ms: 904 },
  { svc: 'kms',      op: 'Decrypt',      fault: 'KMSInternalException', ms: 8 },
  { svc: 'apigw',    op: 'invoke',       fault: '502 BadGateway',    ms: 41 },
  { svc: 'sts',      op: 'AssumeRole',   fault: 'RegionDisabledException', ms: 2 },
  { svc: 'opensrch', op: 'Bulk',         fault: 'circuit_breaking_exception', ms: 67 },
]

const visible = ref<typeof events[number] & { id: number } | null>(null)
let id = 0
let timer: ReturnType<typeof setTimeout> | null = null

function schedule() {
  const wait = 9000 + Math.random() * 14000
  timer = setTimeout(() => {
    const ev = events[Math.floor(Math.random() * events.length)]
    visible.value = { ...ev, id: ++id }
    setTimeout(() => {
      if (visible.value && visible.value.id === id) visible.value = null
    }, 5200)
    schedule()
  }, wait)
}

onMounted(() => {
  // delay first one so it doesn't fire before the boot finishes
  setTimeout(schedule, 6500)
})
onUnmounted(() => { if (timer) clearTimeout(timer) })

function dismiss() { visible.value = null }
</script>

<template>
  <transition name="slide">
    <div v-if="visible" class="toast" role="status">
      <div class="dot"></div>
      <div class="body">
        <div class="hdr">
          <span class="lbl">chaos</span>
          <span class="svc">{{ visible.svc }}.{{ visible.op }}</span>
        </div>
        <div class="msg">
          injected <span class="fault">{{ visible.fault }}</span>
          <span class="lat">+{{ visible.ms }}ms</span>
        </div>
      </div>
      <button class="x" @click="dismiss" aria-label="dismiss">x</button>
    </div>
  </transition>
</template>

<style scoped>
.toast {
  position: fixed;
  right: 18px;
  bottom: 18px;
  z-index: 50;
  display: flex;
  gap: 10px;
  align-items: flex-start;
  max-width: 340px;
  padding: 10px 12px;
  background: #110f0c;
  border: 1px solid var(--awsim-orange);
  border-radius: 8px;
  font-family: var(--awsim-mono);
  box-shadow: 0 14px 40px -12px rgba(255,95,86,0.45);
}
.dot {
  width: 8px; height: 8px;
  margin-top: 5px;
  border-radius: 50%;
  background: #ff5f56;
  box-shadow: 0 0 10px rgba(255,95,86,0.9);
  animation: pulse 1s ease-in-out infinite;
}
@keyframes pulse {
  0%, 100% { opacity: 1; } 50% { opacity: 0.3; }
}
.body { flex: 1; min-width: 0; }
.hdr { display: flex; align-items: baseline; gap: 8px; }
.lbl {
  color: var(--awsim-orange);
  font-size: 10px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.svc { color: #ecdfc0; font-size: 12px; }
.msg { color: #d6c8a6; font-size: 12px; margin-top: 2px; }
.fault { color: #ff8a6b; }
.lat { color: #7a7468; margin-left: 6px; }
.x {
  background: transparent;
  border: none;
  color: #6c6453;
  cursor: pointer;
  font-size: 14px;
  padding: 0 4px;
  line-height: 1;
}
.x:hover { color: #ecdfc0; }

.slide-enter-from { transform: translateY(12px); opacity: 0; }
.slide-enter-active, .slide-leave-active { transition: all 220ms ease; }
.slide-leave-to { transform: translateY(8px); opacity: 0; }
</style>
