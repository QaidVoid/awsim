/**
 * Keyboard shortcut manager — Svelte 5 runes singleton.
 *
 * Supports both single-key shortcuts (`?`, `/`, `t`) and leader-key
 * sequences (`g s` for S3, `g l` for request log, etc.). Keys are
 * detected globally on `window`, but ignored when the user is typing
 * into an input, textarea, select, or contenteditable element.
 *
 * Mod-key chords (Cmd+K, Ctrl+K) are intentionally NOT handled here —
 * those are wired directly in `+layout.svelte` so they stay obvious.
 */

import { browser } from "$app/environment";

const SEQ_TIMEOUT_MS = 1200;

export interface Shortcut {
  /** Space-separated key sequence, e.g. `"g s"` or `"?"`. */
  keys: string;
  description: string;
  category: string;
  action: () => void;
}

class ShortcutManager {
  /** Currently buffered prefix, e.g. "g" — null when idle. Drives a small UI hint. */
  pendingPrefix: string | null = $state(null);

  private shortcuts: Shortcut[] = [];
  private buffer: string[] = [];
  private bufferTimer: ReturnType<typeof setTimeout> | null = null;
  private active = false;
  private handler = (e: KeyboardEvent) => this.handle(e);

  register(list: Shortcut[]) {
    this.shortcuts = list;
  }

  list(): Shortcut[] {
    return this.shortcuts;
  }

  groups(): Map<string, Shortcut[]> {
    const out = new Map<string, Shortcut[]>();
    for (const s of this.shortcuts) {
      if (!out.has(s.category)) out.set(s.category, []);
      out.get(s.category)!.push(s);
    }
    return out;
  }

  start() {
    if (!browser || this.active) return;
    window.addEventListener("keydown", this.handler);
    this.active = true;
  }

  stop() {
    if (!browser || !this.active) return;
    window.removeEventListener("keydown", this.handler);
    this.active = false;
    this.resetBuffer();
  }

  private resetBuffer() {
    this.buffer = [];
    this.pendingPrefix = null;
    if (this.bufferTimer) {
      clearTimeout(this.bufferTimer);
      this.bufferTimer = null;
    }
  }

  private isTypingTarget(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    const tag = target.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
    if (target.isContentEditable) return true;
    // shadcn cmd-k uses [cmdk-input] — typing there should never trigger globals
    if (target.closest("[cmdk-input]")) return true;
    return false;
  }

  private handle(e: KeyboardEvent) {
    // Bail out for system/browser modifier chords so we never steal cmd-k,
    // ctrl-r, alt-tab and friends.
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (this.isTypingTarget(e.target)) return;

    if (e.key === "Escape") {
      if (this.buffer.length) this.resetBuffer();
      return;
    }

    // Single-character keys only (skip Tab, Arrow, F-keys, etc.).
    if (e.key.length !== 1 && e.key !== "?") return;

    const candidate = [...this.buffer, e.key].join(" ");
    const exact = this.shortcuts.find((s) => s.keys === candidate);
    if (exact) {
      e.preventDefault();
      exact.action();
      this.resetBuffer();
      return;
    }

    const prefix = candidate + " ";
    const hasPrefix = this.shortcuts.some((s) => s.keys.startsWith(prefix));
    if (hasPrefix) {
      e.preventDefault();
      this.buffer.push(e.key);
      this.pendingPrefix = candidate;
      if (this.bufferTimer) clearTimeout(this.bufferTimer);
      this.bufferTimer = setTimeout(() => this.resetBuffer(), SEQ_TIMEOUT_MS);
      return;
    }

    // Unknown key — clear any in-flight sequence.
    if (this.buffer.length) this.resetBuffer();
  }
}

export const shortcuts = new ShortcutManager();
