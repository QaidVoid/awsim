<script lang="ts">
	/**
	 * Tiny floating chip shown while a leader-key sequence is in
	 * flight (e.g. after pressing `g`). Disappears when the sequence
	 * resolves or times out (managed by the shortcuts singleton).
	 */
	import { shortcuts } from '$lib/shortcuts.svelte';

	const prefix = $derived(shortcuts.pendingPrefix);
</script>

{#if prefix}
	<div
		class="pointer-events-none fixed bottom-4 left-1/2 z-[100] -translate-x-1/2"
		role="status"
		aria-live="polite"
	>
		<div
			class="flex items-center gap-2 rounded-md border border-border bg-card/90 px-3 py-1.5 shadow-lg shadow-black/40 backdrop-blur"
		>
			<span class="text-[10px] uppercase tracking-wide text-muted-foreground">leader</span>
			<div class="flex items-center gap-1">
				{#each prefix.split(' ') as k (k)}
					<kbd
						class="inline-flex h-5 min-w-[1.25rem] items-center justify-center rounded border border-b-2 border-border bg-muted px-1.5 font-mono text-[10px] leading-none text-foreground"
					>
						{k}
					</kbd>
				{/each}
				<span class="font-mono text-[10px] text-muted-foreground">…</span>
			</div>
		</div>
	</div>
{/if}
