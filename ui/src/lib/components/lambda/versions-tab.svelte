<script lang="ts">
	import {
		listVersionsByFunction,
		publishVersion,
		type LambdaVersion
	} from '$lib/api/lambda';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { toast } from 'svelte-sonner';
	import GitBranch from '@lucide/svelte/icons/git-branch';
	import Tag from '@lucide/svelte/icons/tag';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';

	interface Props {
		functionName: string;
	}

	let { functionName }: Props = $props();

	let versions = $state<LambdaVersion[]>([]);
	let loading = $state(false);
	let publishing = $state(false);
	let description = $state('');
	let lastFn = $state('');

	$effect(() => {
		if (functionName && functionName !== lastFn) {
			lastFn = functionName;
			void load();
		}
	});

	async function load() {
		loading = true;
		try {
			const r = await listVersionsByFunction(functionName);
			versions = r.versions;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to list versions');
		} finally {
			loading = false;
		}
	}

	async function handlePublish() {
		publishing = true;
		try {
			const v = await publishVersion(functionName, description.trim() || undefined);
			toast.success(`Published version ${v.version}`);
			description = '';
			await load();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Publish failed');
		} finally {
			publishing = false;
		}
	}

	function bytesHuman(n: number): string {
		if (!n) return '0 B';
		if (n < 1024) return `${n} B`;
		const units = ['KB', 'MB', 'GB'];
		let v = n / 1024;
		let i = 0;
		while (v >= 1024 && i < units.length - 1) {
			v /= 1024;
			i++;
		}
		return `${v >= 100 ? Math.round(v) : Math.round(v * 10) / 10} ${units[i]}`;
	}

	function formatDate(iso: string): string {
		if (!iso) return '—';
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}

	const columns = [
		{ key: 'version', label: 'Version', mono: true, width: '20%' },
		{ key: 'description', label: 'Description', width: '40%' },
		{ key: 'lastModified', label: 'Modified', width: '25%' },
		{
			key: 'codeSize',
			label: 'Size',
			align: 'right' as const,
			width: '15%'
		}
	];

	let rows = $derived(
		versions.map((v) => ({
			version: v.version,
			description: v.description || '—',
			lastModified: formatDate(v.lastModified),
			codeSize: bytesHuman(v.codeSize)
		}))
	);
</script>

<div class="flex h-full min-h-0 flex-col">
	<section class="border-b border-border bg-background/40 px-4 py-3">
		<div class="flex items-end gap-2">
			<div class="flex-1">
				<Label for="version-desc">Publish a new version</Label>
				<Input
					id="version-desc"
					bind:value={description}
					placeholder="Optional release notes"
					class="mt-1.5"
				/>
			</div>
			<Button type="button" onclick={handlePublish} disabled={publishing}>
				<Tag />
				{publishing ? 'Publishing...' : 'Publish version'}
			</Button>
			<Button
				type="button"
				variant="outline"
				size="icon"
				onclick={load}
				aria-label="Refresh"
			>
				<RefreshCw />
			</Button>
		</div>
	</section>

	<div class="min-h-0 flex-1">
		{#if loading && versions.length === 0}
			<div class="flex h-32 items-center justify-center text-muted-foreground">
				<Loader2 class="size-4 animate-spin" />
			</div>
		{:else if versions.length === 0}
			<div class="p-6">
				<EmptyState
					icon={GitBranch}
					title="No versions yet"
					description="Publish a version to snapshot the current code & config."
				/>
			</div>
		{:else}
			<DataTable {rows} {columns} rowKey={(r) => r.version} />
		{/if}
	</div>
</div>
