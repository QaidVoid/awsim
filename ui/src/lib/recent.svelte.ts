/**
 * Recent navigation history — LRU of the last 10 visited URLs, persisted
 * to localStorage. Used by the command palette so frequent pages are one
 * keystroke away.
 */

import { browser } from "$app/environment";

const STORAGE_KEY = "awsim-recent";
const MAX_RECENT = 10;

function read(): string[] {
  if (!browser) return [];
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed)
      ? parsed.filter((p): p is string => typeof p === "string")
      : [];
  } catch {
    return [];
  }
}

function write(items: string[]) {
  if (!browser) return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(items));
  } catch {
    /* ignore quota errors */
  }
}

class RecentStore {
  items: string[] = $state(read());

  push(path: string) {
    if (!path || path === "/") return;
    const next = [path, ...this.items.filter((p) => p !== path)].slice(
      0,
      MAX_RECENT,
    );
    this.items = next;
    write(next);
  }

  clear() {
    this.items = [];
    write([]);
  }
}

export const recent = new RecentStore();
