<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue'

// "Typical noisy dev workload" we pretend the visitor is running on real AWS
// while they sit on this page. Numbers are intentionally close-to-plausible,
// not audited - this is a vibes-based bankruptcy estimator.
const HOURLY_USD = 4.27 // ~ a small fleet + DDB on-demand + lots of S3 PUTs
const PER_SEC = HOURLY_USD / 3600

const WALLET_USD = 4218.55 // theatrical "remaining runway" before bankruptcy

const start = ref(Date.now())
const now = ref(Date.now())
let raf: number | null = null

const elapsed = computed(() => (now.value - start.value) / 1000)
const cost = computed(() => elapsed.value * PER_SEC)
const runway = computed(() => Math.max(0, WALLET_USD - cost.value))
const ttb = computed(() => {
  // time to bankruptcy at this burn
  const secs = runway.value / PER_SEC
  if (!isFinite(secs) || secs <= 0) return 'NOW'
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  if (d > 0) return `${d}d ${h}h ${m}m`
  if (h > 0) return `${h}h ${m}m`
  return `${m}m`
})

function fmt(n: number) {
  return n.toLocaleString('en-US', { minimumFractionDigits: 4, maximumFractionDigits: 4 })
}

function tick() {
  now.value = Date.now()
  raf = requestAnimationFrame(tick)
}

onMounted(() => { raf = requestAnimationFrame(tick) })
onUnmounted(() => { if (raf) cancelAnimationFrame(raf) })
</script>

<template>
  <div class="bill">
    <div class="row hdr">
      <span class="lbl">// what real aws would have charged you while reading this page</span>
    </div>
    <div class="row big">
      <span class="dollar">$</span>
      <span class="amt">{{ fmt(cost) }}</span>
      <span class="unit">USD</span>
      <span class="pulse"></span>
    </div>
    <div class="row legend">
      <div class="cell">
        <span class="k">burn rate</span>
        <span class="v">${{ HOURLY_USD.toFixed(2) }}/hr</span>
      </div>
      <div class="cell">
        <span class="k">elapsed</span>
        <span class="v">{{ Math.floor(elapsed) }}s</span>
      </div>
      <div class="cell warn">
        <span class="k">time to bankruptcy</span>
        <span class="v">{{ ttb }}</span>
      </div>
      <div class="cell good">
        <span class="k">your awsim bill</span>
        <span class="v">$0.0000</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.bill {
  border: 1px solid var(--awsim-edge);
  border-radius: 10px;
  background:
    radial-gradient(60% 100% at 50% 0%, rgba(255,153,0,0.06), transparent 70%),
    var(--awsim-panel);
  padding: 16px 18px;
  font-family: var(--awsim-mono);
}
.row { display: flex; align-items: baseline; gap: 8px; }
.hdr .lbl { color: #7a7468; font-size: 12px; letter-spacing: 0.02em; }
.big {
  margin: 6px 0 14px;
  align-items: center;
}
.dollar {
  color: var(--awsim-orange);
  font-size: 22px;
  font-weight: 600;
}
.amt {
  font-size: clamp(34px, 5.4vw, 56px);
  font-weight: 600;
  letter-spacing: -0.02em;
  color: #ffd99a;
  text-shadow: 0 0 24px rgba(255,153,0,0.35);
  font-variant-numeric: tabular-nums;
}
.unit { color: #7a7468; font-size: 13px; letter-spacing: 0.12em; }
.pulse {
  display: inline-block;
  width: 8px; height: 8px;
  margin-left: 8px;
  border-radius: 50%;
  background: #ff5f56;
  box-shadow: 0 0 12px rgba(255,95,86,0.8);
  animation: pulse 1.1s ease-in-out infinite;
}
@keyframes pulse {
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.35; transform: scale(0.7); }
}
.legend {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 10px;
}
.cell {
  border: 1px dashed rgba(255,255,255,0.08);
  border-radius: 6px;
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.k {
  color: #7a7468;
  font-size: 10.5px;
  text-transform: uppercase;
  letter-spacing: 0.1em;
}
.v { color: #d6c8a6; font-size: 14px; font-variant-numeric: tabular-nums; }
.cell.warn .v { color: #ff8a6b; }
.cell.good .v { color: #6dcf6a; }

@media (max-width: 720px) {
  .legend { grid-template-columns: repeat(2, 1fr); }
}
</style>
