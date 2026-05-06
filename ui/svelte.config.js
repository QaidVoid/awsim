import adapter from '@sveltejs/adapter-static';

const dev = process.env.NODE_ENV !== 'production';

/** @type {import('@sveltejs/kit').Config} */
const config = {
	compilerOptions: {
		// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
		runes: ({ filename }) => (filename.split(/[/\\]/).includes('node_modules') ? undefined : true)
	},
	kit: {
		// SPA build: emit a static `index.html` and a `200.html` fallback so any
		// client-side route resolves to the same shell. The Rust binary embeds
		// `ui/build/` via rust-embed and serves it under `/_awsim/ui/`.
		adapter: adapter({
			pages: 'build',
			assets: 'build',
			fallback: '200.html',
			precompress: false,
			strict: false
		}),
		paths: {
			// Mount under `/_awsim/ui` so the embedded assets share the admin
			// prefix and never collide with AWS service paths. `vite dev` keeps
			// the empty base so the proxy in `vite.config.ts` works unchanged.
			base: dev ? '' : '/_awsim/ui'
		},
		prerender: {
			// Dynamic routes (e.g. /iam/users/[name]) are hydrated client-side
			// from the SPA fallback — they have no build-time entry, so the
			// prerender crawler never sees them. Skip the warning instead of
			// trying to enumerate every possible parameter.
			handleUnseenRoutes: 'ignore'
		}
	}
};

export default config;
