<script lang="ts">
	import { onMount } from 'svelte';
	import { getTemplate } from '$lib/api/cloudformation';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Copy from '@lucide/svelte/icons/copy';
	import FileCode from '@lucide/svelte/icons/file-code';
	import { toast } from 'svelte-sonner';

	interface Props {
		stackName: string;
	}

	let { stackName }: Props = $props();

	let body = $state<string>('');
	let loading = $state(true);
	let error = $state<string | null>(null);

	function pretty(raw: string): string {
		const trimmed = raw.trim();
		if (!trimmed) return '';
		if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
			try {
				return JSON.stringify(JSON.parse(trimmed), null, 2);
			} catch {
				return trimmed;
			}
		}
		return trimmed;
	}

	async function reload() {
		loading = true;
		error = null;
		try {
			const t = await getTemplate(stackName);
			body = pretty(t.body);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load template';
		} finally {
			loading = false;
		}
	}

	async function copy() {
		try {
			await navigator.clipboard.writeText(body);
			toast.success('Template copied');
		} catch {
			toast.error('Copy failed');
		}
	}

	$effect(() => {
		void stackName;
		reload();
	});

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex shrink-0 items-center justify-between border-b border-border px-4 py-2">
		<span class="text-xs text-muted-foreground">Template body</span>
		<div class="flex items-center gap-2">
			<Button
				type="button"
				size="sm"
				variant="ghost"
				onclick={copy}
				disabled={!body || loading}
				aria-label="Copy template"
			>
				<Copy />
				Copy
			</Button>
			<Button
				type="button"
				size="sm"
				variant="ghost"
				onclick={reload}
				disabled={loading}
				aria-label="Refresh template"
			>
				<RefreshCw class={loading ? 'animate-spin' : ''} />
			</Button>
		</div>
	</div>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && !body}
			<div class="space-y-2 p-4">
				{#each Array(8) as _, i (i)}
					<Skeleton class="h-4 w-full" />
				{/each}
			</div>
		{:else if error}
			<div class="p-6">
				<EmptyState icon={FileCode} title="Template unavailable" description={error} />
			</div>
		{:else if !body}
			<div class="p-6">
				<EmptyState icon={FileCode} title="Empty template" description="No template body returned." />
			</div>
		{:else}
			<pre
				class="m-0 whitespace-pre overflow-auto bg-card p-4 font-mono text-xs leading-relaxed text-foreground"
			>{body}</pre>
		{/if}
	</div>
</div>
