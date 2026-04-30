<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';

	interface Props {
		entries: { key: string; value: string }[];
		keyPlaceholder?: string;
		valuePlaceholder?: string;
		/// Hint shown on the new-row key input (per-IdP-type prompts).
		suggestedKeys?: string[];
		onChange: (entries: { key: string; value: string }[]) => void;
		readonly?: boolean;
	}

	let {
		entries = $bindable([]),
		keyPlaceholder = 'key',
		valuePlaceholder = 'value',
		suggestedKeys = [],
		onChange,
		readonly = false
	}: Props = $props();

	let newKey = $state('');
	let newValue = $state('');

	function add() {
		const k = newKey.trim();
		if (!k) return;
		if (entries.some((e) => e.key === k)) return;
		const next = [...entries, { key: k, value: newValue }];
		entries = next;
		newKey = '';
		newValue = '';
		onChange(next);
	}

	function remove(key: string) {
		const next = entries.filter((e) => e.key !== key);
		entries = next;
		onChange(next);
	}

	function update(key: string, value: string) {
		const next = entries.map((e) => (e.key === key ? { ...e, value } : e));
		entries = next;
		onChange(next);
	}
</script>

<div class="space-y-1.5">
	{#if entries.length === 0}
		<p class="text-xs text-muted-foreground">No entries.</p>
	{:else}
		<ul class="space-y-1.5">
			{#each entries as e (e.key)}
				<li class="grid grid-cols-[10rem_minmax(0,1fr)_auto] items-center gap-2 text-sm">
					<code class="truncate rounded border border-border/60 bg-muted/40 px-2 py-1 font-mono text-xs">
						{e.key}
					</code>
					<Input
						bind:value={e.value}
						oninput={() => update(e.key, e.value)}
						class="h-7 min-w-0 text-xs"
						placeholder={valuePlaceholder}
						disabled={readonly}
					/>
					{#if !readonly}
						<Button
							variant="ghost"
							size="icon-sm"
							onclick={() => remove(e.key)}
							class="text-destructive hover:text-destructive"
							title="Remove"
						>
							<X class="size-3.5" />
						</Button>
					{:else}
						<span></span>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
	{#if !readonly}
		<div class="grid grid-cols-[10rem_minmax(0,1fr)_auto] items-center gap-2 pt-1">
			<Input
				bind:value={newKey}
				placeholder={keyPlaceholder}
				class="h-7 font-mono text-xs"
				list={suggestedKeys.length > 0 ? 'kv-suggested' : undefined}
			/>
			<Input bind:value={newValue} placeholder={valuePlaceholder} class="h-7 min-w-0 text-xs" />
			<Button size="xs" onclick={add} disabled={!newKey.trim()}>
				<Plus class="size-3" />
			</Button>
		</div>
		{#if suggestedKeys.length > 0}
			<datalist id="kv-suggested">
				{#each suggestedKeys as k (k)}
					<option value={k}></option>
				{/each}
			</datalist>
		{/if}
	{/if}
</div>
