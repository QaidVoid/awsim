// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare global {
  namespace App {
    // interface Error {}
    // interface Locals {}
    // interface PageData {}
    // interface PageState {}
    // interface Platform {}
  }

  // Injected by vite.config.ts at build time from the workspace
  // Cargo.toml so the UI version stays glued to the Rust app version.
  const __APP_VERSION__: string;
}

export {};
