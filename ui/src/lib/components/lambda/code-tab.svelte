<script lang="ts">
	import type { LambdaFunctionDetail } from '$lib/api/lambda';
	import { Button } from '$lib/components/ui/button';
	import { EmptyState } from '$lib/components/service';
	import Download from '@lucide/svelte/icons/download';
	import FileArchive from '@lucide/svelte/icons/file-archive';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		detail: LambdaFunctionDetail | null;
		loading: boolean;
	}

	let { detail, loading }: Props = $props();

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
</script>

<div class="flex flex-col gap-4 p-4">
	{#if loading}
		<div class="flex h-32 items-center justify-center text-muted-foreground">
			<Loader2 class="size-4 animate-spin" />
		</div>
	{:else if !detail}
		<EmptyState icon={FileArchive} title="No code information" />
	{:else}
		<section class="rounded-md border border-border bg-card">
			<header class="border-b border-border px-4 py-3">
				<h3 class="text-sm font-medium">Code package</h3>
				<p class="mt-0.5 text-xs text-muted-foreground">
					Deployment ZIP for <span class="font-mono">{detail.configuration.name}</span>
				</p>
			</header>
			<div class="grid grid-cols-2 gap-4 px-4 py-3 text-sm">
				<div>
					<div class="text-xs text-muted-foreground">Code size</div>
					<div class="mt-0.5 font-mono">{bytesHuman(detail.configuration.codeSize)}</div>
				</div>
				<div>
					<div class="text-xs text-muted-foreground">Repository type</div>
					<div class="mt-0.5 font-mono">{detail.code?.repositoryType || '—'}</div>
				</div>
				<div class="col-span-2">
					<div class="text-xs text-muted-foreground">Pre-signed location</div>
					<code class="mt-0.5 block truncate rounded bg-muted px-2 py-1 font-mono text-[11px]">
						{detail.code?.location || '—'}
					</code>
				</div>
			</div>
			<footer class="flex items-center justify-end gap-2 border-t border-border px-4 py-3">
				{#if detail.code?.location}
					<Button
						href={detail.code.location}
						variant="outline"
						size="sm"
						target="_blank"
						rel="noopener"
					>
						<Download />
						Download ZIP
					</Button>
				{:else}
					<span class="text-xs text-muted-foreground">Code not available</span>
				{/if}
			</footer>
		</section>

		<section class="rounded-md border border-border bg-card">
			<header class="border-b border-border px-4 py-3">
				<h3 class="text-sm font-medium">File listing</h3>
				<p class="mt-0.5 text-xs text-muted-foreground">
					LocalStack does not expose ZIP contents over the API. Download to inspect.
				</p>
			</header>
			<div class="px-4 py-6 text-center text-xs text-muted-foreground">
				Use the download button above to fetch the package.
			</div>
		</section>
	{/if}
</div>
