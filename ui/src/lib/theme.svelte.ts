/**
 * Theme store — Svelte 5 runes-based picker backed by localStorage.
 *
 * Variants compose with the existing `dark` Tailwind variant: every dark
 * theme applies the `dark` class (so `dark:` utilities still work) plus
 * an optional `theme-{id}` class that overrides the colour tokens. The
 * pre-paint script in `app.html` applies the saved theme before
 * hydration so there is no flash on first load.
 */

import { browser } from "$app/environment";

export type Theme = "light" | "dark" | "midnight" | "slate" | "solarized";

export interface ThemeMeta {
  id: Theme;
  label: string;
  mode: "dark" | "light";
  /** Three-colour preview swatch shown in the picker. */
  swatch: { bg: string; fg: string; accent: string };
}

export const THEMES: ThemeMeta[] = [
  {
    id: "dark",
    label: "Default Dark",
    mode: "dark",
    swatch: {
      bg: "oklch(0.13 0.01 270)",
      fg: "oklch(0.95 0.005 270)",
      accent: "oklch(0.7 0.18 35)",
    },
  },
  {
    id: "midnight",
    label: "Midnight",
    mode: "dark",
    swatch: {
      bg: "oklch(0.09 0.02 280)",
      fg: "oklch(0.94 0.01 280)",
      accent: "oklch(0.7 0.21 295)",
    },
  },
  {
    id: "slate",
    label: "Slate",
    mode: "dark",
    swatch: {
      bg: "oklch(0.16 0.012 235)",
      fg: "oklch(0.95 0.005 235)",
      accent: "oklch(0.72 0.15 220)",
    },
  },
  {
    id: "solarized",
    label: "Solarized Dark",
    mode: "dark",
    swatch: {
      bg: "oklch(0.22 0.025 195)",
      fg: "oklch(0.86 0.04 75)",
      accent: "oklch(0.7 0.13 195)",
    },
  },
  {
    id: "light",
    label: "Light",
    mode: "light",
    swatch: {
      bg: "oklch(0.99 0.005 270)",
      fg: "oklch(0.18 0.01 270)",
      accent: "oklch(0.7 0.18 35)",
    },
  },
];

export const VARIANT_CLASSES: readonly string[] = [
  "theme-midnight",
  "theme-slate",
  "theme-solarized",
];

const STORAGE_KEY = "awsim-theme";
const LAST_DARK_KEY = "awsim-theme-last-dark";

function isTheme(v: string | null): v is Theme {
  return (
    v === "light" || v === "dark" || v === "midnight" || v === "slate" || v === "solarized"
  );
}

function readInitial(): Theme {
  if (!browser) return "dark";
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    return isTheme(saved) ? saved : "dark";
  } catch {
    return "dark";
  }
}

function applyToDom(t: Theme) {
  if (!browser) return;
  const root = document.documentElement;
  root.classList.remove("dark", "light", ...VARIANT_CLASSES);
  const meta = THEMES.find((m) => m.id === t) ?? THEMES[0];
  root.classList.add(meta.mode === "dark" ? "dark" : "light");
  if (t !== "dark" && t !== "light") root.classList.add(`theme-${t}`);
}

class ThemeStore {
  current: Theme = $state<Theme>(readInitial());

  get isDark(): boolean {
    return THEMES.find((m) => m.id === this.current)?.mode === "dark";
  }

  /**
   * Cycle dark <-> light. Going dark restores the most recent dark
   * variant the user picked (so `t` is a fast, non-destructive toggle).
   */
  toggle() {
    if (this.isDark) {
      try {
        localStorage.setItem(LAST_DARK_KEY, this.current);
      } catch {
        /* ignore */
      }
      this.set("light");
    } else {
      let restore: Theme = "dark";
      try {
        const v = localStorage.getItem(LAST_DARK_KEY);
        if (isTheme(v) && v !== "light") restore = v;
      } catch {
        /* ignore */
      }
      this.set(restore);
    }
  }

  set(t: Theme) {
    this.current = t;
    if (browser) {
      try {
        localStorage.setItem(STORAGE_KEY, t);
      } catch {
        /* ignore */
      }
    }
    applyToDom(t);
  }
}

export const theme = new ThemeStore();
