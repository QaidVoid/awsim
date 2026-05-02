import { page } from '$app/state';
import { afterNavigate, replaceState } from '$app/navigation';
import { browser } from '$app/environment';

/**
 * Syncs tab state to URL (`?tab=`, write-only for sharing) and localStorage
 * (read/write for persistence across back-button and reload).
 *
 * Priority on load: URL param > localStorage > defaultTab.
 * On change: writes to both URL (replaceState) and localStorage.
 *
 * Usage:
 * ```svelte
 * <script>
 *   let active = $state(
 *     useTab('sqs', ['queues', 'dlq'] as const, 'queues', {
 *       get: () => active,
 *       set: (v) => (active = v)
 *     })
 *   );
 * </script>
 * <Tabs bind:value={active}>
 * ```
 */
export function useTab<T extends string>(
	storeKey: string,
	validTabs: readonly T[],
	defaultTab: T,
	accessor: { get: () => T; set: (v: T) => void },
): T {
	let routerReady = $state(false);

	afterNavigate(() => {
		routerReady = true;
	});

	function readInitial(): T {
		const urlTab = page.url.searchParams.get('tab') as T | null;
		if (urlTab && validTabs.includes(urlTab)) return urlTab;
		if (browser) {
			const stored = localStorage.getItem(`tab:${storeKey}`) as T | null;
			if (stored && validTabs.includes(stored)) return stored;
		}
		return defaultTab;
	}

	$effect(() => {
		if (!browser) return;
		const v = accessor.get();
		localStorage.setItem(`tab:${storeKey}`, v);
		if (!routerReady) return;
		const url = new URL(window.location.href);
		if (url.searchParams.get('tab') === v) return;
		url.searchParams.set('tab', v);
		replaceState(url.toString(), {});
	});

	return readInitial();
}
