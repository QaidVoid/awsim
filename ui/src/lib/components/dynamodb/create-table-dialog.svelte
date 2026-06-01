<script lang="ts">
	import { toast } from 'svelte-sonner';
	import {
		createTable,
		type ScalarType,
		type SecondaryIndexInput,
	} from '$lib/api/dynamodb';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Switch } from '$lib/components/ui/switch';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Badge } from '$lib/components/ui/badge';
	import { Separator } from '$lib/components/ui/separator';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';

	interface Props {
		open: boolean;
		onClose: () => void;
		onCreated: (name: string) => void;
	}

	let { open = $bindable(false), onClose, onCreated }: Props = $props();

	type ProjectionType = 'KEYS_ONLY' | 'INCLUDE' | 'ALL';

	const SCALAR_TYPE_LABELS: Record<ScalarType, string> = {
		S: 'String',
		N: 'Number',
		B: 'Binary'
	};
	const PROJECTION_LABELS: Record<ProjectionType, string> = {
		KEYS_ONLY: 'KEYS_ONLY',
		INCLUDE: 'INCLUDE',
		ALL: 'ALL',
	};

	interface IndexRow {
		_k: number;
		indexName: string;
		hashKey: string;
		hashKeyType: ScalarType;
		rangeKey: string;
		rangeKeyType: ScalarType;
		projectionType: ProjectionType;
		includeAttrsText: string;
	}

	let name = $state('');
	let pkName = $state('');
	let pkType = $state<ScalarType>('S');
	let skName = $state('');
	let skType = $state<ScalarType>('S');
	let deletionProtection = $state(false);
	let gsis = $state<IndexRow[]>([]);
	let lsis = $state<IndexRow[]>([]);
	let saving = $state(false);
	let error = $state<string | null>(null);
	let rowKeyCounter = 1;

	$effect(() => {
		if (!open) {
			name = '';
			pkName = '';
			pkType = 'S';
			skName = '';
			skType = 'S';
			deletionProtection = false;
			gsis = [];
			lsis = [];
			saving = false;
			error = null;
		}
	});

	function blankIndex(): IndexRow {
		return {
			_k: rowKeyCounter++,
			indexName: '',
			hashKey: '',
			hashKeyType: 'S',
			rangeKey: '',
			rangeKeyType: 'S',
			projectionType: 'ALL',
			includeAttrsText: '',
		};
	}

	function addGsi() {
		gsis = [...gsis, blankIndex()];
	}
	function removeGsi(i: number) {
		gsis = gsis.filter((_, j) => j !== i);
	}
	// LSIs share the table's PK as their hash key (AWS rule). Seed the
	// row with the current PK name/type so the user can't accidentally
	// type a divergent one.
	function addLsi() {
		const row = blankIndex();
		row.hashKey = pkName.trim();
		row.hashKeyType = pkType;
		lsis = [...lsis, row];
	}
	function removeLsi(i: number) {
		lsis = lsis.filter((_, j) => j !== i);
	}

	function splitIncludeAttrs(text: string): string[] {
		return text
			.split(/[,\s]+/)
			.map((s) => s.trim())
			.filter((s) => s.length > 0);
	}

	function toWire(rows: IndexRow[]): SecondaryIndexInput[] {
		return rows.map((r) => ({
			indexName: r.indexName.trim(),
			hashKey: r.hashKey.trim(),
			hashKeyType: r.hashKeyType,
			rangeKey: r.rangeKey.trim() || undefined,
			rangeKeyType: r.rangeKey.trim() ? r.rangeKeyType : undefined,
			projectionType: r.projectionType,
			nonKeyAttributes:
				r.projectionType === 'INCLUDE'
					? splitIncludeAttrs(r.includeAttrsText)
					: undefined,
		}));
	}

	// Centralised validation so the dialog can both disable Create
	// and surface the first failing reason inline.
	let validationError = $derived.by<string | null>(() => {
		if (!name.trim() || !pkName.trim()) {
			return 'Table and partition key names are required.';
		}
		if (skName.trim() === pkName.trim() && skName.trim() !== '') {
			return 'Sort key must differ from partition key.';
		}
		const allNames = new Set<string>();
		for (const idx of [...gsis, ...lsis]) {
			const n = idx.indexName.trim();
			if (!n) return 'Every index needs a name.';
			if (allNames.has(n)) return `Duplicate index name: ${n}.`;
			allNames.add(n);
			if (!idx.hashKey.trim()) {
				return `Index ${n}: hash key is required.`;
			}
		}
		for (const idx of lsis) {
			if (idx.hashKey.trim() !== pkName.trim()) {
				return `LSI ${idx.indexName.trim()}: hash key must equal the table's partition key.`;
			}
			if (!idx.rangeKey.trim()) {
				return `LSI ${idx.indexName.trim()}: range key is required.`;
			}
			if (idx.rangeKey.trim() === skName.trim() && skName.trim() !== '') {
				return `LSI ${idx.indexName.trim()}: range key must differ from the table's sort key.`;
			}
		}
		return null;
	});

	let canSubmit = $derived(!saving && !validationError);

	async function submit() {
		if (!canSubmit) {
			error = validationError;
			return;
		}
		saving = true;
		error = null;
		try {
			await createTable({
				name: name.trim(),
				partitionKey: pkName.trim(),
				partitionKeyType: pkType,
				sortKey: skName.trim() || undefined,
				sortKeyType: skType,
				deletionProtectionEnabled: deletionProtection,
				globalSecondaryIndexes: gsis.length > 0 ? toWire(gsis) : undefined,
				localSecondaryIndexes: lsis.length > 0 ? toWire(lsis) : undefined,
			});
			toast.success(`Created table ${name.trim()}`);
			onCreated(name.trim());
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Failed to create table';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}
</script>

{#snippet indexRow(idx: IndexRow, i: number, kind: 'gsi' | 'lsi', onRemove: (i: number) => void)}
	<div class="flex flex-col gap-2 rounded-md border border-border p-3">
		<div class="flex items-center justify-between gap-2">
			<div class="flex items-center gap-2">
				<Badge variant="outline" class="text-[10px]">{kind === 'gsi' ? 'GSI' : 'LSI'}</Badge>
				<span class="font-mono text-xs text-muted-foreground">#{i + 1}</span>
			</div>
			<Button
				variant="ghost"
				size="icon"
				type="button"
				onclick={() => onRemove(i)}
				aria-label="Remove index"
			>
				<Trash2 class="size-4 text-rose-600" />
			</Button>
		</div>
		<div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
			<div class="flex flex-col gap-1">
				<Label class="text-[10px] uppercase text-muted-foreground">Index name</Label>
				<Input bind:value={idx.indexName} placeholder="byStatus" class="font-mono text-xs" />
			</div>
			<div class="flex flex-col gap-1">
				<Label class="text-[10px] uppercase text-muted-foreground">Projection</Label>
				<Select
					type="single"
					value={idx.projectionType}
					onValueChange={(v) => (idx.projectionType = v as ProjectionType)}
				>
					<SelectTrigger class="w-full">{PROJECTION_LABELS[idx.projectionType]}</SelectTrigger>
					<SelectContent>
						<SelectItem value="KEYS_ONLY" label="KEYS_ONLY">KEYS_ONLY</SelectItem>
						<SelectItem value="INCLUDE" label="INCLUDE">INCLUDE</SelectItem>
						<SelectItem value="ALL" label="ALL">ALL</SelectItem>
					</SelectContent>
				</Select>
			</div>
		</div>
		<div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
			<div class="flex flex-col gap-1">
				<Label class="text-[10px] uppercase text-muted-foreground">
					Hash key{kind === 'lsi' ? ' (= table PK)' : ''}
				</Label>
				<div class="flex gap-2">
					<Input
						bind:value={idx.hashKey}
						placeholder="status"
						class="flex-1 font-mono text-xs"
						disabled={kind === 'lsi'}
					/>
					<Select
						type="single"
						value={idx.hashKeyType}
						onValueChange={(v) => (idx.hashKeyType = v as ScalarType)}
						disabled={kind === 'lsi'}
					>
						<SelectTrigger class="w-24">{SCALAR_TYPE_LABELS[idx.hashKeyType]}</SelectTrigger>
						<SelectContent>
							<SelectItem value="S" label="String">String</SelectItem>
							<SelectItem value="N" label="Number">Number</SelectItem>
							<SelectItem value="B" label="Binary">Binary</SelectItem>
						</SelectContent>
					</Select>
				</div>
			</div>
			<div class="flex flex-col gap-1">
				<Label class="text-[10px] uppercase text-muted-foreground">
					Range key{kind === 'lsi' ? '' : ' (optional)'}
				</Label>
				<div class="flex gap-2">
					<Input
						bind:value={idx.rangeKey}
						placeholder={kind === 'lsi' ? 'createdAt' : 'leave blank for none'}
						class="flex-1 font-mono text-xs"
					/>
					<Select
						type="single"
						value={idx.rangeKeyType}
						onValueChange={(v) => (idx.rangeKeyType = v as ScalarType)}
					>
						<SelectTrigger class="w-24">{SCALAR_TYPE_LABELS[idx.rangeKeyType]}</SelectTrigger>
						<SelectContent>
							<SelectItem value="S" label="String">String</SelectItem>
							<SelectItem value="N" label="Number">Number</SelectItem>
							<SelectItem value="B" label="Binary">Binary</SelectItem>
						</SelectContent>
					</Select>
				</div>
			</div>
		</div>
		{#if idx.projectionType === 'INCLUDE'}
			<div class="flex flex-col gap-1">
				<Label class="text-[10px] uppercase text-muted-foreground">Non-key attributes</Label>
				<Textarea
					bind:value={idx.includeAttrsText}
					placeholder="email, displayName"
					rows={2}
					class="font-mono text-xs"
				/>
			</div>
		{/if}
	</div>
{/snippet}

<Dialog bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<DialogContent class="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
		<DialogHeader>
			<DialogTitle>Create table</DialogTitle>
			<DialogDescription>
				Pay-per-request billing. LSIs can only be defined here — they're locked once the table
				exists.
			</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="flex flex-col gap-1.5">
				<Label for="ddb-name">Table name</Label>
				<Input id="ddb-name" bind:value={name} placeholder="my-table" autocomplete="off" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="ddb-pk">Partition key</Label>
				<div class="flex gap-2">
					<Input id="ddb-pk" bind:value={pkName} placeholder="id" class="flex-1" />
					<Select
						type="single"
						value={pkType}
						onValueChange={(v) => (pkType = v as ScalarType)}
					>
						<SelectTrigger aria-label="Partition key type" class="w-32">
							{SCALAR_TYPE_LABELS[pkType]}
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="S" label="String">String</SelectItem>
							<SelectItem value="N" label="Number">Number</SelectItem>
							<SelectItem value="B" label="Binary">Binary</SelectItem>
						</SelectContent>
					</Select>
				</div>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="ddb-sk">Sort key (optional)</Label>
				<div class="flex gap-2">
					<Input id="ddb-sk" bind:value={skName} placeholder="leave blank if none" class="flex-1" />
					<Select
						type="single"
						value={skType}
						onValueChange={(v) => (skType = v as ScalarType)}
					>
						<SelectTrigger aria-label="Sort key type" class="w-32">
							{SCALAR_TYPE_LABELS[skType]}
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="S" label="String">String</SelectItem>
							<SelectItem value="N" label="Number">Number</SelectItem>
							<SelectItem value="B" label="Binary">Binary</SelectItem>
						</SelectContent>
					</Select>
				</div>
			</div>
			<div class="flex items-start justify-between gap-4 rounded-md border border-border p-3">
				<div>
					<Label for="ddb-deletion-protection" class="text-sm">Deletion protection</Label>
					<p class="mt-0.5 text-xs text-muted-foreground">
						Reject DeleteTable requests until disabled. Toggle off later via the Schema tab.
					</p>
				</div>
				<Switch id="ddb-deletion-protection" bind:checked={deletionProtection} />
			</div>

			<Separator />

			<div class="flex items-center justify-between">
				<Label class="flex items-center gap-1.5">
					Global secondary indexes
					<Badge variant="outline" class="text-[10px]">add/remove anytime</Badge>
				</Label>
				<Button variant="outline" size="sm" type="button" onclick={addGsi}>
					<Plus class="size-3.5" />
					<span class="ml-1">Add GSI</span>
				</Button>
			</div>
			{#if gsis.length === 0}
				<p class="text-xs text-muted-foreground">
					No GSIs yet. You can also add them later from the Indexes tab.
				</p>
			{:else}
				<div class="flex flex-col gap-2">
					{#each gsis as g, i (g._k)}
						{@render indexRow(g, i, 'gsi', removeGsi)}
					{/each}
				</div>
			{/if}

			<div class="flex items-center justify-between">
				<Label class="flex items-center gap-1.5">
					Local secondary indexes
					<Badge variant="outline" class="text-[10px]">create-time only</Badge>
				</Label>
				<Button
					variant="outline"
					size="sm"
					type="button"
					onclick={addLsi}
					disabled={!pkName.trim() || lsis.length >= 5}
				>
					<Plus class="size-3.5" />
					<span class="ml-1">Add LSI</span>
				</Button>
			</div>
			{#if !pkName.trim()}
				<p class="text-xs text-muted-foreground">
					Set the partition key first; LSIs share it as their hash key.
				</p>
			{:else if lsis.length === 0}
				<p class="text-xs text-muted-foreground">
					No LSIs. AWS caps LSIs at 5 and only lets you define them here.
				</p>
			{:else}
				<div class="flex flex-col gap-2">
					{#each lsis as l, i (l._k)}
						{@render indexRow(l, i, 'lsi', removeLsi)}
					{/each}
				</div>
			{/if}

			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{:else if validationError && (name || pkName)}
				<p class="text-xs text-amber-600">{validationError}</p>
			{/if}
			<DialogFooter>
				<Button type="button" variant="outline" onclick={onClose} disabled={saving}>
					Cancel
				</Button>
				<Button type="submit" disabled={!canSubmit}>
					{#if saving}
						<Loader2 class="size-3.5 animate-spin" />
					{/if}
					Create
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
