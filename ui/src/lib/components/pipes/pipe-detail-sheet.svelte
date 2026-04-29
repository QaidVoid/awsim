<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import PlayIcon from '@lucide/svelte/icons/play';
	import PauseIcon from '@lucide/svelte/icons/pause';
	import { toast } from 'svelte-sonner';
	import {
		describePipe,
		deletePipe,
		startPipe,
		stopPipe,
		type Pipe,
		type PipeState
	} from '$lib/api/pipes';

	interface Props {
		open: boolean;
		name: string | null;
		onOpenChange: (open: boolean) => void;
		onChanged?: () => void;
	}

	let { open, name, onOpenChange, onChanged }: Props = $props();

	let pipe = $state<Pipe | null>(null);
	let loading = $state(false);
	let busy = $state(false);

	$effect(() => {
		if (open && name) {
			void load(name);
		} else if (!open) {
			pipe = null;
		}
	});

	async function load(n: string) {
		loading = true;
		try {
			pipe = await describePipe(n);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load pipe');
		} finally {
			loading = false;
		}
	}

	async function handleStart() {
		if (!pipe) return;
		busy = true;
		try {
			await startPipe(pipe.name);
			toast.success(`Started ${pipe.name}.`);
			await load(pipe.name);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to start');
		} finally {
			busy = false;
		}
	}

	async function handleStop() {
		if (!pipe) return;
		busy = true;
		try {
			await stopPipe(pipe.name);
			toast.success(`Stopped ${pipe.name}.`);
			await load(pipe.name);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to stop');
		} finally {
			busy = false;
		}
	}

	async function handleDelete() {
		if (!pipe) return;
		if (!confirm(`Delete pipe "${pipe.name}"?`)) return;
		busy = true;
		try {
			await deletePipe(pipe.name);
			toast.success('Pipe deleted.');
			onChanged?.();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		} finally {
			busy = false;
		}
	}

	function stateColor(s: PipeState): string {
		if (s === 'RUNNING') return 'text-green-500';
		if (s === 'STOPPED') return 'text-muted-foreground';
		if (s === 'CREATE_FAILED') return 'text-destructive';
		return 'text-amber-500';
	}

	function timestamp(t?: number): string {
		if (!t) return '—';
		return new Date(t * 1000).toLocaleString();
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-xl">
		<SheetHeader>
			<SheetTitle>Pipe details</SheetTitle>
			<SheetDescription>
				{#if name}
					<span class="font-mono text-xs">{name}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4">
			{#if loading}
				<p class="text-sm text-muted-foreground">Loading…</p>
			{:else if pipe}
				<div class="flex flex-wrap items-center gap-2">
					<Badge
						variant="outline"
						class={`h-5 px-2 text-[10px] ${stateColor(pipe.currentState)}`}
					>
						{pipe.currentState}
					</Badge>
					{#if pipe.currentState !== pipe.desiredState}
						<Badge variant="outline" class="h-5 px-2 text-[10px] text-amber-500">
							→ {pipe.desiredState}
						</Badge>
					{/if}
				</div>

				{#if pipe.stateReason}
					<div class="rounded-md border border-border bg-muted/40 p-2 text-xs">
						<div class="font-semibold text-muted-foreground">State reason</div>
						<div class="mt-1 font-mono">{pipe.stateReason}</div>
					</div>
				{/if}

				<div class="space-y-1.5">
					<div class="text-xs font-semibold text-muted-foreground">Source</div>
					<div class="font-mono text-xs break-all">{pipe.source}</div>
				</div>

				<div class="space-y-1.5">
					<div class="text-xs font-semibold text-muted-foreground">Target</div>
					<div class="font-mono text-xs break-all">{pipe.target}</div>
				</div>

				{#if pipe.enrichment}
					<div class="space-y-1.5">
						<div class="text-xs font-semibold text-muted-foreground">Enrichment</div>
						<div class="font-mono text-xs break-all">{pipe.enrichment}</div>
					</div>
				{/if}

				<div class="space-y-1.5">
					<div class="text-xs font-semibold text-muted-foreground">Role ARN</div>
					<div class="font-mono text-xs break-all">{pipe.roleArn}</div>
				</div>

				{#if pipe.description}
					<div class="space-y-1.5">
						<div class="text-xs font-semibold text-muted-foreground">Description</div>
						<div class="text-sm">{pipe.description}</div>
					</div>
				{/if}

				{#if pipe.sourceParameters}
					<div class="space-y-1.5">
						<div class="text-xs font-semibold text-muted-foreground">Source parameters</div>
						<pre class="rounded-md border border-border bg-muted/40 p-2 font-mono text-[11px] whitespace-pre-wrap">{JSON.stringify(pipe.sourceParameters, null, 2)}</pre>
					</div>
				{/if}

				{#if pipe.targetParameters}
					<div class="space-y-1.5">
						<div class="text-xs font-semibold text-muted-foreground">Target parameters</div>
						<pre class="rounded-md border border-border bg-muted/40 p-2 font-mono text-[11px] whitespace-pre-wrap">{JSON.stringify(pipe.targetParameters, null, 2)}</pre>
					</div>
				{/if}

				<div class="grid grid-cols-2 gap-3 text-xs">
					<div>
						<div class="font-semibold text-muted-foreground">Created</div>
						<div>{timestamp(pipe.creationTime)}</div>
					</div>
					<div>
						<div class="font-semibold text-muted-foreground">Last modified</div>
						<div>{timestamp(pipe.lastModifiedTime)}</div>
					</div>
				</div>

				<div class="flex flex-wrap items-center gap-2 border-t border-border pt-3">
					{#if pipe.currentState === 'RUNNING'}
						<Button size="sm" variant="outline" onclick={handleStop} disabled={busy}>
							<PauseIcon />
							Stop
						</Button>
					{:else}
						<Button size="sm" onclick={handleStart} disabled={busy}>
							<PlayIcon />
							Start
						</Button>
					{/if}
					<Button size="sm" variant="ghost" onclick={handleDelete} disabled={busy}>
						<Trash2Icon class="text-destructive" />
						Delete
					</Button>
				</div>
			{/if}
		</div>
	</SheetContent>
</Sheet>
