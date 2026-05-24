<script setup lang="ts">
const stats = [
  { k: 'binary',      v: '< 30 MB',  note: 'single static binary' },
  { k: 'cold start',  v: '< 500 ms', note: 'measured on a laptop' },
  { k: 'idle ram',    v: '< 10 MiB', note: 'before you hit it' },
  { k: 'services',    v: '52',       note: 'one endpoint, one port' },
  { k: 'cloud deps',  v: '0',        note: 'air-gapped on purpose' },
  { k: 'aws account', v: 'none',     note: 'no signup, no card' },
  { k: 'monthly bill',v: '$0',       note: 'forever' },
  { k: 'iam ops',     v: '26',       note: 'all condition operators' },
]
</script>

<template>
  <div class="receipts">
    <div class="head">
      <span class="prefix">$</span> awsim stat --all
    </div>
    <div class="grid">
      <div v-for="s in stats" :key="s.k" class="cell">
        <div class="k">{{ s.k }}</div>
        <div class="v">{{ s.v }}</div>
        <div class="n">{{ s.note }}</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.receipts {
  border: 1px solid var(--awsim-edge);
  border-radius: 10px;
  background: var(--awsim-panel);
  padding: 18px;
  font-family: var(--awsim-mono);
}
.head {
  font-size: 13px;
  color: #d6c8a6;
  margin-bottom: 14px;
}
.prefix { color: var(--awsim-orange); }
.grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 0;
  border: 1px dashed rgba(255,255,255,0.08);
  border-radius: 6px;
  overflow: hidden;
}
.cell {
  padding: 14px;
  border-right: 1px dashed rgba(255,255,255,0.06);
  border-bottom: 1px dashed rgba(255,255,255,0.06);
}
.cell:nth-child(4n) { border-right: none; }
.cell:nth-child(n+5) { /* second row */ }
.cell:nth-last-child(-n+4) { border-bottom: none; }
.k {
  color: #7a7468;
  font-size: 10.5px;
  text-transform: uppercase;
  letter-spacing: 0.12em;
}
.v {
  color: #ffd99a;
  font-size: 22px;
  font-weight: 600;
  margin: 4px 0 2px;
  letter-spacing: -0.01em;
}
.n { color: #6c6453; font-size: 11px; }

@media (max-width: 880px) {
  .grid { grid-template-columns: repeat(2, 1fr); }
  .cell:nth-child(4n) { border-right: 1px dashed rgba(255,255,255,0.06); }
  .cell:nth-child(2n) { border-right: none; }
  .cell:nth-last-child(-n+2) { border-bottom: none; }
}
</style>
