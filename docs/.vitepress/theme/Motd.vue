<script setup lang="ts">
import { ref } from 'vue'
import AsciiLogo from './components/AsciiLogo.vue'
import BootTerminal from './components/BootTerminal.vue'
import BillCounter from './components/BillCounter.vue'
import ServiceConstellation from './components/ServiceConstellation.vue'
import Receipts from './components/Receipts.vue'
import ChaosToast from './components/ChaosToast.vue'

const install = `docker run --rm -p 4566:4566 -p 4567:4567 \\
  -e AWSIM_HTTPS_PORT=4567 \\
  ghcr.io/qaidvoid/awsim:latest
# https://aws.qaidvoid.dev:4567  (pinned 127.0.0.1, real Let's Encrypt cert)`

const sdk = `export AWS_ENDPOINT_URL=https://aws.qaidvoid.dev:4567
export AWS_ACCESS_KEY_ID=awsim-admin
export AWS_SECRET_ACCESS_KEY=awsim-admin
aws s3 mb s3://buckets-are-free-now`

const copied = ref<string | null>(null)
async function copy(text: string, tag: string) {
  try {
    await navigator.clipboard.writeText(text)
    copied.value = tag
    setTimeout(() => { if (copied.value === tag) copied.value = null }, 1400)
  } catch {}
}
</script>

<template>
  <div class="motd-home">
    <div class="scanlines" aria-hidden="true"></div>

    <header class="topbar">
      <div class="brand">
        <span class="prompt">root@awsim:~#</span>
        <span class="path">cat /etc/motd</span>
        <span class="blink">_</span>
      </div>
      <nav class="nav">
        <a href="/guide/getting-started.html">guide</a>
        <a href="/services/">services</a>
        <a href="https://github.com/QaidVoid/awsim" target="_blank" rel="noopener">github</a>
      </nav>
    </header>

    <section class="hero">
      <div class="hero-left">
        <AsciiLogo />
        <h1 class="tagline">
          The AWS dev environment that
          <span class="strike">phones home</span>
          <span class="accent">doesn't.</span>
        </h1>
        <p class="pitch">
          One static binary. 52 services on one port. No tokens, no signups,
          no surprise bill at the end of the month. Your laptop is the cloud now.
        </p>

        <div class="cta">
          <a class="btn primary" href="/guide/getting-started.html">
            <span class="caret">&gt;</span> get started
          </a>
          <a class="btn ghost" href="/services/">browse services</a>
          <a class="btn ghost" href="https://github.com/QaidVoid/awsim" target="_blank" rel="noopener">
            star on github
          </a>
        </div>

        <div class="snippet">
          <div class="snip-hdr">
            <span>one-liner</span>
            <button @click="copy(install, 'install')">
              {{ copied === 'install' ? 'copied' : 'copy' }}
            </button>
          </div>
          <pre><code>{{ install }}</code></pre>
        </div>
      </div>

      <div class="hero-right">
        <BootTerminal />
      </div>
    </section>

    <section class="band">
      <BillCounter />
    </section>

    <section class="band tight">
      <Receipts />
    </section>

    <section class="band">
      <ServiceConstellation />
    </section>

    <section class="band">
      <div class="diff">
        <header class="diff-hdr">
          // diff against the alternatives
        </header>
        <div class="diff-grid">
          <div class="col">
            <h3>real AWS</h3>
            <ul>
              <li>- credit card</li>
              <li>- "free tier" with a footnote</li>
              <li>- IAM that fails open in test</li>
              <li>- VPC tax for talking to yourself</li>
              <li>- gone when the wifi dies</li>
            </ul>
          </div>
          <div class="col">
            <h3>that other emulator</h3>
            <ul>
              <li>- Python + a JVM somewhere</li>
              <li>- pro tier behind a paywall</li>
              <li>- Docker required</li>
              <li>- IAM mostly cosmetic</li>
              <li>- slow cold start</li>
            </ul>
          </div>
          <div class="col mine">
            <h3>awsim</h3>
            <ul>
              <li>+ single binary, sub-500ms boot</li>
              <li>+ real SigV4 + 26 IAM operators</li>
              <li>+ chaos engine baked in</li>
              <li>+ rolling cost estimator</li>
              <li>+ MIT / Apache-2.0, forever</li>
            </ul>
          </div>
        </div>
      </div>
    </section>

    <section class="band">
      <div class="snippet wide">
        <div class="snip-hdr">
          <span>point any sdk at it</span>
          <button @click="copy(sdk, 'sdk')">
            {{ copied === 'sdk' ? 'copied' : 'copy' }}
          </button>
        </div>
        <pre><code>{{ sdk }}</code></pre>
      </div>
    </section>

    <footer class="footnote">
      <div class="row">
        <span>// MIT / Apache-2.0</span>
        <span class="spacer"></span>
        <span>built with rust, sqlite, and spite for AWS console latency</span>
      </div>
      <pre class="ascii-foot">+-------------------------------------------------------+
| no telemetry. no analytics. no "we noticed you" mail. |
+-------------------------------------------------------+</pre>
    </footer>

    <ChaosToast />
  </div>
</template>

<style scoped>
.motd-home {
  position: relative;
  max-width: 1180px;
  margin: 0 auto;
  padding: 28px 22px 80px;
  color: #d6c8a6;
  font-family: var(--awsim-sans);
}
.scanlines {
  position: fixed;
  inset: 0;
  pointer-events: none;
  z-index: 1;
  background: repeating-linear-gradient(
    0deg,
    rgba(255,255,255,0.018) 0 1px,
    transparent 1px 3px
  );
  mix-blend-mode: overlay;
}

.topbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-family: var(--awsim-mono);
  font-size: 12.5px;
  padding-bottom: 18px;
  border-bottom: 1px dashed rgba(255,255,255,0.08);
  margin-bottom: 28px;
}
.brand .prompt { color: var(--awsim-orange); }
.brand .path { color: #d6c8a6; margin-left: 6px; }
.brand .blink {
  color: var(--awsim-orange);
  animation: blink 1s steps(1) infinite;
}
@keyframes blink { 50% { opacity: 0; } }
.nav { display: flex; gap: 18px; }
.nav a {
  color: #9a8f78;
  text-decoration: none;
  transition: color 120ms ease;
}
.nav a:hover { color: var(--awsim-orange); }

.hero {
  display: grid;
  grid-template-columns: minmax(0, 1.05fr) minmax(0, 0.95fr);
  gap: 36px;
  align-items: start;
  margin-bottom: 36px;
}
.hero-left, .hero-right { min-width: 0; }
.tagline {
  font-family: var(--awsim-sans);
  font-size: clamp(28px, 4vw, 44px);
  line-height: 1.08;
  font-weight: 600;
  letter-spacing: -0.02em;
  color: #f3e9cb;
  margin: 18px 0 14px;
}
.tagline .strike {
  position: relative;
  color: #6c6453;
}
.tagline .strike::after {
  content: '';
  position: absolute;
  left: -2%; right: -2%;
  top: 54%;
  height: 3px;
  background: #ff5f56;
  transform: rotate(-2deg);
}
.tagline .accent { color: var(--awsim-orange); }

.pitch {
  color: #b3a78a;
  font-size: 16px;
  line-height: 1.55;
  max-width: 52ch;
  margin-bottom: 22px;
}

.cta { display: flex; flex-wrap: wrap; gap: 10px; margin-bottom: 22px; }
.btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-family: var(--awsim-mono);
  font-size: 13px;
  padding: 10px 16px;
  border-radius: 6px;
  text-decoration: none;
  border: 1px solid transparent;
  transition: all 120ms ease;
}
.btn.primary {
  background: var(--awsim-orange);
  color: #1a0f00;
  font-weight: 600;
}
.btn.primary:hover { transform: translateY(-1px); box-shadow: 0 8px 20px -10px rgba(255,153,0,0.8); }
.btn.ghost {
  color: #d6c8a6;
  border-color: rgba(255,255,255,0.12);
}
.btn.ghost:hover { border-color: var(--awsim-orange); color: var(--awsim-orange); }
.btn .caret { color: inherit; }

.snippet {
  border: 1px solid var(--awsim-edge);
  border-radius: 8px;
  background: var(--awsim-panel);
  font-family: var(--awsim-mono);
  overflow: hidden;
}
.snippet.wide pre { font-size: 13px; }
.snip-hdr {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 12px;
  background: #161410;
  border-bottom: 1px solid var(--awsim-edge);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.1em;
  color: #7a7468;
}
.snip-hdr button {
  background: transparent;
  border: 1px solid rgba(255,255,255,0.1);
  border-radius: 4px;
  color: #d6c8a6;
  padding: 3px 10px;
  font-family: inherit;
  font-size: 11px;
  cursor: pointer;
  letter-spacing: 0.05em;
}
.snip-hdr button:hover { border-color: var(--awsim-orange); color: var(--awsim-orange); }
.snippet pre {
  margin: 0;
  padding: 14px 16px;
  font-size: 13px;
  color: #ecdfc0;
  background: transparent;
  white-space: pre-wrap;
}

.band { margin: 28px 0; }
.band.tight { margin-top: 16px; }

.diff {
  border: 1px solid var(--awsim-edge);
  border-radius: 10px;
  background: var(--awsim-panel);
  padding: 18px;
  font-family: var(--awsim-mono);
}
.diff-hdr { color: #7a7468; font-size: 12px; margin-bottom: 14px; }
.diff-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 12px;
}
.col {
  background: #0e0d0b;
  border: 1px solid #1c1a17;
  border-radius: 6px;
  padding: 14px 14px 12px;
}
.col h3 {
  margin: 0 0 10px;
  font-size: 13px;
  color: #d6c8a6;
  letter-spacing: 0.03em;
}
.col ul { list-style: none; padding: 0; margin: 0; }
.col li {
  font-size: 12.5px;
  line-height: 1.55;
  color: #b3a78a;
  font-family: var(--awsim-mono);
}
.col.mine { border-color: rgba(255,153,0,0.4); background: #15110b; }
.col.mine h3 { color: var(--awsim-orange); }
.col.mine li { color: #ecdfc0; }

.footnote {
  margin-top: 40px;
  padding-top: 18px;
  border-top: 1px dashed rgba(255,255,255,0.08);
  font-family: var(--awsim-mono);
  color: #6c6453;
  font-size: 11.5px;
}
.footnote .row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 12px;
}
.footnote .spacer { flex: 1; }
.ascii-foot {
  margin: 0;
  font-size: 11px;
  color: #6c6453;
  white-space: pre;
  overflow: hidden;
}

@media (max-width: 920px) {
  .hero { grid-template-columns: 1fr; gap: 22px; }
  .diff-grid { grid-template-columns: 1fr; }
  .topbar { flex-direction: column; align-items: flex-start; gap: 10px; }
  .nav { flex-wrap: wrap; }
}
</style>
