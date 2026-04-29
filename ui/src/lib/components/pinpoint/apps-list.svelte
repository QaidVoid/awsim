<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';
	import { listApps, createApp, deleteApp, type App } from '$lib/api/pinpoint';

	interface Props {
		selectedId: string | null;
		onSelect: (a: App) => void;
		onChanged?: () => void;
	}

	let { selectedId, onSelect, onChanged }: Props = $props();

	let apps = $state<App[]>([]);
	let loading = $state(false);
	let newName = $state('');
	let creating = $state(false);

	async function load() {
		loading = true;
		try {
			apps = await listApps();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load apps');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void load();
	});

	async function create() {
		if (!newName.trim()) return toast.error('App name is required.');
		creating = true;
		try {
			const a = await createApp(newName.trim());
			toast.success(`Created app "${a.name}".`);
			newName = '';
			await load();
			onSelect(a);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create app');
		} finally {
			creating = false;
		}
	}

	async function remove(a: App, ev: MouseEvent) {
		ev.stopPropagation();
		if (!confirm(`Delete app "${a.name}"? Cascades to endpoints/segments/campaigns.`)) return;
		try {
			await deleteApp(a.id);
			toast.success('App deleted.');
			await load();
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center justify-between border-b border-border px-3 py-2">
		<h3 class="text-xs font-semibold uppercase text-muted-foreground">Apps</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
		</Button>
	</div>
	<div class="space-y-2 border-b border-border p-3">
		<Input bind:value={newName} placeholder="app name" class="h-8 text-xs" />
		<Button size="sm" class="w-full" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'New app'}
		</Button>
	</div>
	<div class="min-h-0 flex-1 overflow-y-auto">
		{#if apps.length === 0 && !loading}
			<p class="p-3 text-xs text-muted-foreground">No apps yet.</p>
		{:else}
			{#each apps as a (a.id)}
				<button
					type="button"
					onclick={() => onSelect(a)}
					class={`flex w-full items-start justify-between gap-2 border-b border-border px-3 py-2 text-left text-sm hover:bg-muted/40 ${
						selectedId === a.id ? 'bg-muted/40' : ''
					}`}
				>
					<div class="flex flex-col gap-0.5">
						<span class="font-mono text-xs">{a.name}</span>
						<span class="font-mono text-[10px] text-muted-foreground">{a.id}</span>
					</div>
					<Button variant="ghost" size="xs" onclick={(e) => remove(a, e)}>
						<Trash2Icon class="text-destructive" />
					</Button>
				</button>
			{/each}
		{/if}
	</div>
</div>
