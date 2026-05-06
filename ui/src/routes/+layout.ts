// Static SPA build: every page resolves to the same shell, hydrated on the
// client. Required for the adapter-static fallback so we don't have to know
// every dynamic route at build time.
export const ssr = false;
export const prerender = true;
export const trailingSlash = 'always';
