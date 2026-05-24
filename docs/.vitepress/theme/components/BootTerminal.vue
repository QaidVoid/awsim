<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'

const lines = [
  { tag: 'sys',  text: '$ awsim --https-port 4567' },
  { tag: 'ok',   text: 'bind 0.0.0.0:4566 ....... ok (3ms)' },
  { tag: 'ok',   text: 'tls listener :4567 ...... ok (12ms)' },
  { tag: 'ok',   text: 'load 52 services ........ ok (41ms)' },
  { tag: 'ok',   text: 'sqlite WAL warmup ....... ok (18ms)' },
  { tag: 'ok',   text: 'iam policy engine ....... ok (7ms)' },
  { tag: 'warn', text: 'internet check .......... skipped' },
  { tag: 'warn', text: 'aws credentials ......... not required' },
  { tag: 'warn', text: 'monthly bill ............ $0.00' },
  { tag: 'ok',   text: 'cold start .............. 472ms' },
  { tag: 'sys',  text: '>> ready. :4566 / :4567' },
]

const visible = ref<typeof lines>([])
const typing = ref('')
const cursor = ref(true)
const done = ref(false)

let lineTimer: ReturnType<typeof setTimeout> | null = null
let charTimer: ReturnType<typeof setTimeout> | null = null
let blinkTimer: ReturnType<typeof setInterval> | null = null

function typeLine(idx: number) {
  if (idx >= lines.length) {
    done.value = true
    return
  }
  const ln = lines[idx]
  let i = 0
  typing.value = ''
  const step = () => {
    if (i <= ln.text.length) {
      typing.value = ln.text.slice(0, i)
      i++
      charTimer = setTimeout(step, 8 + Math.random() * 18)
    } else {
      visible.value.push(ln)
      typing.value = ''
      lineTimer = setTimeout(() => typeLine(idx + 1), 140)
    }
  }
  step()
}

onMounted(() => {
  blinkTimer = setInterval(() => { cursor.value = !cursor.value }, 480)
  lineTimer = setTimeout(() => typeLine(0), 350)
})

onUnmounted(() => {
  if (lineTimer) clearTimeout(lineTimer)
  if (charTimer) clearTimeout(charTimer)
  if (blinkTimer) clearInterval(blinkTimer)
})
</script>

<template>
  <div class="boot">
    <div class="chrome">
      <span class="dot dot-r"></span>
      <span class="dot dot-y"></span>
      <span class="dot dot-g"></span>
      <span class="title">tty/awsim - boot.log</span>
    </div>
    <div class="screen">
      <div v-for="(ln, i) in visible" :key="i" :class="['ln', 'tag-' + ln.tag]">
        <span class="tag">[{{ ln.tag.toUpperCase().padEnd(4) }}]</span>
        <span class="msg">{{ ln.text }}</span>
      </div>
      <div v-if="!done" class="ln tag-typing">
        <span class="tag">[....]</span>
        <span class="msg">{{ typing }}<span :class="['cur', cursor ? 'on' : '']">_</span></span>
      </div>
      <div v-else class="ln tag-prompt">
        <span class="tag">$&gt;</span>
        <span class="msg">_<span :class="['cur', cursor ? 'on' : '']"></span></span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.boot {
  border: 1px solid var(--awsim-edge);
  border-radius: 10px;
  background: var(--awsim-panel);
  box-shadow: 0 0 0 1px rgba(255,153,0,0.08), 0 20px 60px -30px rgba(0,0,0,0.7);
  overflow: hidden;
  font-family: var(--awsim-mono);
  min-width: 0;
  max-width: 100%;
}
.chrome {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px;
  background: linear-gradient(180deg, #1a1815, #100f0d);
  border-bottom: 1px solid var(--awsim-edge);
}
.dot { width: 10px; height: 10px; border-radius: 50%; display: inline-block; }
.dot-r { background: #ff5f56; }
.dot-y { background: #ffbd2e; }
.dot-g { background: #27c93f; }
.title {
  margin-left: 8px;
  color: #7a7468;
  font-size: 11px;
  letter-spacing: 0.04em;
}
.screen {
  padding: 18px 18px 22px;
  min-height: 290px;
  background:
    repeating-linear-gradient(0deg, rgba(255,255,255,0.012) 0 1px, transparent 1px 3px),
    #0b0a08;
  font-size: 13.5px;
  line-height: 1.55;
  color: #d6c8a6;
}
.ln {
  display: grid;
  grid-template-columns: 62px minmax(0, 1fr);
  gap: 8px;
  white-space: pre;
  min-width: 0;
}
.msg {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tag { color: #6c6453; }
.tag-ok .tag { color: #6dcf6a; }
.tag-warn .tag { color: var(--awsim-orange); }
.tag-sys .tag { color: #5cc2ff; }
.tag-typing .tag { color: #6c6453; }
.tag-prompt .tag { color: var(--awsim-orange); }
.tag-sys .msg { color: #e9dfc4; }
.cur {
  display: inline-block;
  width: 8px;
  margin-left: 2px;
  background: transparent;
  color: var(--awsim-orange);
}
.cur.on { color: var(--awsim-orange); }
.cur:not(.on) { color: transparent; }

@media (max-width: 640px) {
  .screen { font-size: 12px; padding: 14px; }
  .ln { grid-template-columns: 56px 1fr; }
}
</style>
