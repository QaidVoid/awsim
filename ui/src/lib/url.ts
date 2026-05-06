import { resolve } from '$app/paths';

/**
 * Prefix an app-relative path with the configured `paths.base` using
 * SvelteKit's `resolve()` helper.
 *
 * SvelteKit doesn't auto-prepend the base to literal hrefs / `goto()`
 * arguments — `<a href="/seed">` resolves to the URL-absolute `/seed`,
 * which on the production build (mounted at `/_awsim/ui`) lands outside
 * the SPA. Pipe every internal navigation through this helper so the
 * base lives in exactly one place.
 *
 * `path` should start with `/`. Returns the empty-prefix string in dev
 * (`base = ''`) and `/_awsim/ui/seed` in production.
 */
export function route(path: string): string {
	// `resolve` is strictly typed against the app's known pathnames. We
	// thread arbitrary catalog strings through it, so the cast widens
	// the input to `string` — keeps one helper that works for both
	// static and dynamic paths without enumerating route IDs at every
	// call site.
	return (resolve as (p: string) => string)(path);
}

/**
 * True when the real browser pathname matches the given app path or sits
 * under it. Wraps the path in `route()` so callers can keep using
 * unprefixed app paths from places like `services-catalog.ts`.
 */
export function isActiveRoute(pathname: string, appPath: string): boolean {
	const target = route(appPath);
	return pathname === target || pathname.startsWith(target + '/');
}
