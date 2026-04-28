<script lang="ts">
	/**
	 * Keyboard shortcut cheat sheet — opened with `?`. Shows every
	 * registered shortcut grouped by category, plus a hint about the
	 * leader-key convention.
	 */
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogHeader,
		DialogTitle,
	} from '$lib/components/ui/dialog';
	import { shortcuts } from '$lib/shortcuts.svelte';

	interface Props {
		open: boolean;
	}

	let { open = $bindable() }: Props = $props();

	const grouped = $derived(shortcuts.groups());
	const KBD = 'inline-flex h-5 min-w-[1.25rem] items-center justify-center rounded border border-b-2 border-border bg-muted px-1.5 font-mono text-[10px] leading-none text-foreground';
</script>

<Dialog bind:open>
	<DialogContent class="max-w-xl">
		<DialogHeader>
			<DialogTitle>Keyboard shortcuts</DialogTitle>
			<DialogDescription>
				Press <kbd class={KBD}>?</kbd> any time to open this list. Sequences like
				<kbd class={KBD}>g</kbd>
				<kbd class={KBD}>s</kbd> are typed in order with a short timeout between keys.
			</DialogDescription>
		</DialogHeader>

		<div class="grid max-h-[60vh] grid-cols-1 gap-4 overflow-y-auto pr-1 sm:grid-cols-2">
			{#each [...grouped.entries()] as [category, items] (category)}
				<section>
					<h3 class="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
						{category}
					</h3>
					<ul class="space-y-1">
						{#each items as s (s.keys)}
							<li class="flex items-center justify-between gap-3 text-sm">
								<span class="text-foreground">{s.description}</span>
								<span class="flex shrink-0 items-center gap-1">
									{#each s.keys.split(' ') as k (k)}
										<kbd class={KBD}>{k}</kbd>
									{/each}
								</span>
							</li>
						{/each}
					</ul>
				</section>
			{/each}
		</div>

		<p class="text-[11px] text-muted-foreground">
			Shortcuts are ignored while typing in inputs. Use <kbd class={KBD}>Esc</kbd> to cancel an in-flight sequence.
		</p>
	</DialogContent>
</Dialog>
