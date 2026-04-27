/**
 * Theme store — Svelte 5 runes-based dark/light toggle backed by
 * localStorage. The pre-paint script in app.html applies the saved
 * theme class before hydration so there is no flash on first load.
 */

import { browser } from "$app/environment";

export type Theme = "dark" | "light";

const STORAGE_KEY = "awsim-theme";

function readInitial(): Theme {
  if (!browser) return "dark";
  const saved = localStorage.getItem(STORAGE_KEY);
  return saved === "light" ? "light" : "dark";
}

function applyToDom(theme: Theme) {
  if (!browser) return;
  document.documentElement.classList.remove("dark", "light");
  document.documentElement.classList.add(theme);
}

class ThemeStore {
  current: Theme = $state<Theme>(readInitial());

  get isDark() {
    return this.current === "dark";
  }

  toggle() {
    this.set(this.current === "dark" ? "light" : "dark");
  }

  set(theme: Theme) {
    this.current = theme;
    if (browser) {
      localStorage.setItem(STORAGE_KEY, theme);
    }
    applyToDom(theme);
  }
}

export const theme = new ThemeStore();
