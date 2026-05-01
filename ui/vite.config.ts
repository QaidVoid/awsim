import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// Pull the app version from the workspace Cargo.toml so the UI never
// drifts behind a Rust-side bump. Match `[workspace.package]` →
// `version = "X.Y.Z"`. Tiny regex avoids adding a TOML parser dep
// just for this single read.
function readWorkspaceVersion(): string {
	try {
		const cargoToml = readFileSync(resolve(__dirname, '..', 'Cargo.toml'), 'utf-8');
		const m = cargoToml.match(
			/^\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m
		);
		return m?.[1] ?? '0.0.0';
	} catch {
		return '0.0.0';
	}
}

const APP_VERSION = readWorkspaceVersion();

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	define: {
		__APP_VERSION__: JSON.stringify(APP_VERSION)
	},
	server: {
		proxy: {
			'/_awsim': 'http://localhost:4566'
		}
	}
});
