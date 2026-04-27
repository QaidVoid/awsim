<script lang="ts">
	import { onMount } from 'svelte';
	import { listLexicons, getLexicon, type Lexicon, type LexiconDetail } from '$lib/api/polly';
	import { DataTable, EmptyState } from '$lib/components/service';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription,
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import BookOpenIcon from '@lucide/svelte/icons/book-open';
	import { toast } from 'svelte-sonner';

	let rows = $state<Lexicon[]>([]);
	let loading = $state(true);
	let sheetOpen = $state(false);
	let detail = $state<LexiconDetail | null>(null);
	let detailLoading = $state(false);

	onMount(load);

	async function load() {
		loading = true;
		try {
			rows = await listLexicons();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load lexicons');
		} finally {
			loading = false;
		}
	}

	async function open(row: Lexicon) {
		detail = null;
		sheetOpen = true;
		detailLoading = true;
		try {
			detail = await getLexicon(row.name);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load lexicon');
			sheetOpen = false;
		} finally {
			detailLoading = false;
		}
	}
</script>

{#snippet alphabetCell(row: Lexicon)}
	{#if row.alphabet}
		<Badge variant="outline" class="h-4 px-1 text-[10px]">{row.alphabet}</Badge>
	{:else}
		<span class="text-[10px] text-muted-foreground">—</span>
	{/if}
{/snippet}

{#snippet langCell(row: Lexicon)}
	<span class="text-xs">{row.languageCode ?? '—'}</span>
{/snippet}

{#snippet sizeCell(row: Lexicon)}
	<span class="font-mono text-xs">{row.size ?? 0}</span>
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<div class="text-xs text-muted-foreground">
			{rows.length} lexicon{rows.length === 1 ? '' : 's'}
		</div>
		<Button variant="outline" size="sm" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name' },
			{ key: 'alphabet', label: 'Alphabet', cell: alphabetCell },
			{ key: 'languageCode', label: 'Language', cell: langCell },
			{ key: 'size', label: 'Size (bytes)', cell: sizeCell },
		]}
		rowKey={(r) => r.name}
		onRowClick={open}
	>
		{#snippet empty()}
			<EmptyState
				icon={BookOpenIcon}
				title="No lexicons"
				description="Pronunciation lexicons let you customize how Polly pronounces specific words."
			/>
		{/snippet}
	</DataTable>
</div>

<Sheet open={sheetOpen} onOpenChange={(o) => (sheetOpen = o)}>
	<SheetContent side="right" class="w-full sm:max-w-lg">
		<SheetHeader>
			<SheetTitle>Lexicon</SheetTitle>
			<SheetDescription>
				{#if detail}
					<span class="font-mono text-xs">{detail.name}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
			{#if detailLoading}
				<p class="text-xs text-muted-foreground">Loading…</p>
			{:else if detail}
				<dl class="grid grid-cols-[120px_1fr] gap-x-3 gap-y-1.5 text-xs">
					<dt class="text-muted-foreground">Alphabet</dt>
					<dd>{detail.alphabet ?? '—'}</dd>
					<dt class="text-muted-foreground">Language</dt>
					<dd>{detail.languageCode ?? '—'}</dd>
					<dt class="text-muted-foreground">Lexemes</dt>
					<dd>{detail.lexemesCount ?? 0}</dd>
					<dt class="text-muted-foreground">Size</dt>
					<dd>{detail.size ?? 0} bytes</dd>
					<dt class="text-muted-foreground">Last modified</dt>
					<dd>{detail.lastModified ?? '—'}</dd>
				</dl>

				<section>
					<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">Content</h3>
					<pre
						class="max-h-[50vh] overflow-auto rounded-md border border-border bg-muted/40 p-3 text-xs whitespace-pre-wrap break-words">{detail.content ||
							'(empty)'}</pre>
				</section>
			{/if}
		</div>

		<div class="flex justify-end gap-2 border-t border-border px-4 py-3">
			<Button variant="outline" onclick={() => (sheetOpen = false)}>Close</Button>
		</div>
	</SheetContent>
</Sheet>
