<script lang="ts">
	import type { S3Object, S3CommonPrefix } from '$lib/api/s3';
	import { formatBytes, formatTimestamp } from '$lib/api/s3';
	import { Button } from '$lib/components/ui/button';
	import { EmptyState } from '$lib/components/service';
	import Folder from '@lucide/svelte/icons/folder';
	import FileText from '@lucide/svelte/icons/file-text';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Inbox from '@lucide/svelte/icons/inbox';

	interface Props {
		bucket: string;
		prefix: string;
		objects: S3Object[];
		commonPrefixes: S3CommonPrefix[];
		loading: boolean;
		onNavigate: (prefix: string) => void;
		onSelectObject: (obj: S3Object) => void;
		onDeleteObject: (obj: S3Object) => void;
		onRefresh: () => void;
	}

	let {
		bucket,
		prefix,
		objects,
		commonPrefixes,
		loading,
		onNavigate,
		onSelectObject,
		onDeleteObject,
		onRefresh
	}: Props = $props();

	let breadcrumbs = $derived.by(() => {
		const parts = prefix.split('/').filter(Boolean);
		const crumbs = [{ label: bucket, pfx: '' }];
		let built = '';
		for (const part of parts) {
			built += part + '/';
			crumbs.push({ label: part, pfx: built });
		}
		return crumbs;
	});

	let isEmpty = $derived(objects.length === 0 && commonPrefixes.length === 0);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div
		class="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-background/40 px-4 py-2"
	>
		<nav class="flex min-w-0 flex-wrap items-center gap-1 text-sm">
			{#each breadcrumbs as crumb, i (crumb.pfx)}
				{#if i > 0}
					<ChevronRight class="size-3 shrink-0 text-muted-foreground" />
				{/if}
				<button
					type="button"
					onclick={() => onNavigate(crumb.pfx)}
					class="rounded px-1 py-0.5 font-mono text-xs transition-colors hover:bg-muted {i ===
					breadcrumbs.length - 1
						? 'text-foreground'
						: 'text-muted-foreground hover:text-foreground'}"
				>
					{crumb.label}
				</button>
			{/each}
		</nav>
		<Button variant="ghost" size="icon-sm" onclick={onRefresh} aria-label="Refresh">
			{#if loading}
				<Loader2 class="size-3.5 animate-spin" />
			{:else}
				<RefreshCw class="size-3.5" />
			{/if}
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-hidden">
		{#if isEmpty && !loading}
			<div class="flex h-full items-center justify-center p-6">
				<EmptyState
					icon={Inbox}
					title={prefix ? 'Folder is empty' : 'Bucket is empty'}
					description="Drop files below to upload."
				/>
			</div>
		{:else}
			<div class="h-full overflow-auto">
				<table class="w-full text-xs">
					<thead
						class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm"
					>
						<tr>
							<th class="px-3 py-2 text-left font-medium text-muted-foreground">Name</th>
							<th class="w-28 px-3 py-2 text-right font-medium text-muted-foreground">Size</th>
							<th class="w-44 px-3 py-2 text-left font-medium text-muted-foreground">Modified</th>
							<th class="w-12"></th>
						</tr>
					</thead>
					<tbody>
						{#each commonPrefixes as cp (cp.prefix)}
							<tr
								class="cursor-pointer border-b border-border/40 hover:bg-muted/40"
								onclick={() => onNavigate(cp.prefix)}
							>
								<td class="px-3 py-1.5">
									<span class="inline-flex items-center gap-1.5 font-mono">
										<Folder class="size-3.5 text-amber-500" />
										{cp.prefix.slice(prefix.length).replace(/\/$/, '') || '/'}
									</span>
								</td>
								<td class="px-3 py-1.5 text-right text-muted-foreground">—</td>
								<td class="px-3 py-1.5 text-muted-foreground">—</td>
								<td></td>
							</tr>
						{/each}
						{#each objects as obj (obj.key)}
							<tr
								class="cursor-pointer border-b border-border/40 hover:bg-muted/40"
								onclick={() => onSelectObject(obj)}
							>
								<td class="px-3 py-1.5">
									<span class="inline-flex items-center gap-1.5 font-mono break-all">
										<FileText class="size-3.5 shrink-0 text-muted-foreground" />
										{obj.key.slice(prefix.length) || obj.key}
									</span>
								</td>
								<td class="px-3 py-1.5 text-right font-mono text-muted-foreground">
									{formatBytes(obj.size)}
								</td>
								<td class="px-3 py-1.5 font-mono text-muted-foreground">
									{formatTimestamp(obj.lastModified)}
								</td>
								<td class="px-2 py-1.5">
									<Button
										variant="ghost"
										size="icon-xs"
										aria-label="Delete object"
										onclick={(e: MouseEvent) => {
											e.stopPropagation();
											onDeleteObject(obj);
										}}
									>
										<Trash2 class="size-3 text-destructive" />
									</Button>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</div>
