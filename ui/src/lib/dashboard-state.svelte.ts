/**
 * Module-scope dashboard state — the single source of truth for the
 * SSE event ring buffer and pause control. A module-level singleton
 * means the request-stream component, KPI cards, service status list
 * and insights panel all observe the same buffer without any prop
 * drilling.
 *
 * Wiring:
 *   - The home page (`+page.svelte`) calls `dashboardState.connect()`
 *     once on mount and `dashboardState.disconnect()` on teardown.
 *   - Components that need data simply read `dashboardState.events`,
 *     `dashboardState.paused`, `dashboardState.connectionStatus` —
 *     they are plain `$state` and trigger reactivity automatically.
 */

import { browser } from "$app/environment";
import type { RequestEvent } from "./events";

const MAX_EVENTS = 500;
const RECONNECT_DELAY_MS = 2000;
// Coalesce incoming SSE messages and apply them on a fixed cadence. A
// busy emulator (a test suite hammering endpoints) can emit hundreds of
// events per second; reassigning `events` per message invalidated every
// dashboard consumer (KPIs, rps, both stream tables) that many times a
// second. Flushing in batches caps reactive churn at ~8/s regardless of
// load while staying well under the threshold a human perceives as lag.
const FLUSH_INTERVAL_MS = 120;

export type ConnectionStatus = "connecting" | "open" | "closed" | "paused";

class DashboardState {
  events: RequestEvent[] = $state([]);
  paused: boolean = $state(false);
  connectionStatus: ConnectionStatus = $state("closed");

  private es: EventSource | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  /** Reference count so multiple components can mount/unmount safely. */
  private refs = 0;
  /** Events received since the last flush, in arrival (oldest-first) order. */
  private pending: RequestEvent[] = [];
  private flushTimer: ReturnType<typeof setInterval> | null = null;

  connect() {
    this.refs++;
    if (!browser) return;
    if (this.es) return;
    this.open();
  }

  disconnect() {
    this.refs = Math.max(0, this.refs - 1);
    if (this.refs === 0) {
      this.close();
    }
  }

  togglePause() {
    this.paused = !this.paused;
    if (this.paused) {
      this.connectionStatus = "paused";
    } else if (this.es && this.es.readyState === EventSource.OPEN) {
      this.connectionStatus = "open";
    } else if (browser && !this.es) {
      this.open();
    }
  }

  clear() {
    this.events = [];
  }

  /** Average requests-per-second over the trailing `windowSecs` seconds. */
  rps(windowSecs = 5, now: number = Date.now() / 1000): number {
    if (this.events.length === 0) return 0;
    const cutoff = now - windowSecs;
    let count = 0;
    for (const evt of this.events) {
      if (evt.ts >= cutoff) count++;
      else break;
    }
    return count / windowSecs;
  }

  private open() {
    if (!browser) return;
    this.connectionStatus = this.paused ? "paused" : "connecting";
    try {
      this.es = new EventSource("/_awsim/events");
    } catch {
      this.scheduleReconnect();
      return;
    }
    this.es.onopen = () => {
      if (!this.paused) this.connectionStatus = "open";
    };
    this.es.onmessage = (e: MessageEvent) => {
      if (this.paused) return;
      try {
        this.pending.push(JSON.parse(e.data) as RequestEvent);
      } catch {
        /* ignore malformed payload */
      }
      if (this.flushTimer === null) {
        this.flushTimer = setInterval(() => this.flush(), FLUSH_INTERVAL_MS);
      }
    };
    this.es.onerror = () => {
      this.close();
      this.scheduleReconnect();
    };
  }

  /**
   * Drain buffered events into `events` in one reassignment. Pending is
   * oldest-first (arrival order); the buffer reverses onto the front so
   * the newest event stays at index 0. Self-cancels its interval once
   * the backlog clears so an idle stream costs nothing.
   */
  private flush() {
    if (this.pending.length === 0) {
      if (this.flushTimer !== null) {
        clearInterval(this.flushTimer);
        this.flushTimer = null;
      }
      return;
    }
    const batch = this.pending;
    this.pending = [];
    const next = batch.reverse().concat(this.events);
    if (next.length > MAX_EVENTS) next.length = MAX_EVENTS;
    this.events = next;
  }

  private close() {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.flushTimer !== null) {
      clearInterval(this.flushTimer);
      this.flushTimer = null;
    }
    this.pending = [];
    if (this.es) {
      this.es.close();
      this.es = null;
    }
    if (!this.paused) this.connectionStatus = "closed";
  }

  private scheduleReconnect() {
    if (!browser) return;
    if (this.refs === 0) return;
    if (this.reconnectTimer) return;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (this.refs > 0) this.open();
    }, RECONNECT_DELAY_MS);
  }
}

export const dashboardState = new DashboardState();
