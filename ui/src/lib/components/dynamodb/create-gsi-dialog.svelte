<script lang="ts">
	import { toast } from 'svelte-sonner';
	import {
		createGsi,
		type ScalarType,
		type TableDetail,
	} from '$lib/api/dynamodb';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle,
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { Textarea } from '$lib/components/ui/textarea';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	type ProjectionType = 'KEYS_ONLY' | 'INCLUDE' | 'ALL';

	interface Props {
		open: boolean;
		detail: TableDetail;
		onClose: () => void;
		onCreated: () => void | Promise<void>;
	}

	let { open = $bindable(false), detail, onClose, onCreated }: Props = $props();

	const SCALAR_TYPE_LABELS: Record<ScalarType, string> = {
		S: 'String',
		N: 'Number',
		B: 'Binary',
	};
	const PROJECTION_LABELS: Record<ProjectionType, string> = {
		KEYS_ONLY: 'KEYS_ONLY (index keys + table keys only)',
		INCLUDE: 'INCLUDE (index keys + selected attrs)',
		ALL: 'ALL (full item copy)',
	};

	let indexName = $state('');
	let hashKey = $state('');
	let hashKeyType = $state<ScalarType>('S');
	let rangeKey = $state('');
	let rangeKeyType = $state<ScalarType>('S');
	let projectionType = $state<ProjectionType>('ALL');
	// Raw text the user types into the NonKeyAttributes box; split on
	// commas/whitespace at submit time. Stored as a string so partial
	// input survives keystrokes.
	let includeAttrsText = $state('');
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) {
			indexName = '';
			hashKey = '';
			hashKeyType = 'S';
			rangeKey = '';
			rangeKeyType = 'S';
			projectionType = 'ALL';
			includeAttrsText = '';
			saving = false;
			error = null;
		}
	});

	// If the chosen hash/range key is already in AttributeDefinitions,
	// surface its type so the user can't pick a conflicting one.
	let existingAttrType = $derived.by((): Record<string, ScalarType> => {
		const out: Record<string, ScalarType> = {};
		for (const a of detail.attributeDefinitions) {
			out[a.attributeName] = a.attributeType;
		}
		return out;
	});
	let hashTypeLocked = $derived(existingAttrType[hashKey.trim()] != null);
	let rangeTypeLocked = $derived(
		rangeKey.trim() !== '' && existingAttrType[rangeKey.trim()] != null,
	);

	$effect(() => {
		const locked = existingAttrType[hashKey.trim()];
		if (locked != null) hashKeyType = locked;
	});
	$effect(() => {
		const locked = existingAttrType[rangeKey.trim()];
		if (locked != null) rangeKeyType = locked;
	});

	let nameError = $derived.by<string | null>(() => {
		const n = indexName.trim();
		if (!n) return 'Index name is required.';
		if (detail.globalSecondaryIndexes.some((g) => g.indexName === n)) {
			return `An index named '${n}' already exists.`;
		}
		if (detail.localSecondaryIndexes.some((l) => l.indexName === n)) {
			return `An LSI named '${n}' already exists.`;
		}
		return null;
	});
	let hashError = $derived(hashKey.trim() ? null : 'Hash key is required.');
	let rangeError = $derived.by<string | null>(() => {
		const r = rangeKey.trim();
		if (!r) return null;
		if (r === hashKey.trim()) return 'Range key must differ from hash key.';
		return null;
	});

	let canSubmit = $derived(
		!saving && !nameError && !hashError && !rangeError && indexName.trim() && hashKey.trim(),
	);

	function splitIncludeAttrs(): string[] {
		return includeAttrsText
			.split(/[,\s]+/)
			.map((s) => s.trim())
			.filter((s) => s.length > 0);
	}

	async function submit() {
		if (!canSubmit) return;
		saving = true;
		error = null;
		try {
			await createGsi(
				detail.name,
				{
					indexName: indexName.trim(),
					hashKey: hashKey.trim(),
					hashKeyType,
					rangeKey: rangeKey.trim() || undefined,
					rangeKeyType: rangeKey.trim() ? rangeKeyType : undefined,
					projectionType,
					nonKeyAttributes:
						projectionType === 'INCLUDE' ? splitIncludeAttrs() : undefined,
				},
				detail.attributeDefinitions,
			);
			toast.success(`Created GSI ${indexName.trim()}`);
			await onCreated();
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Failed to create GSI';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>Add global secondary index</DialogTitle>
			<DialogDescription>
				Add a GSI to <code class="font-mono">{detail.name}</code>. The backfill runs synchronously
				against existing items, so the index is queryable as soon as this dialog closes.
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
				<Label for="gsi-name">Index name</Label>
				<Input
					id="gsi-name"
					bind:value={indexName}
					placeholder="byStatus"
					autocomplete="off"
					class="font-mono"
				/>
				{#if nameError && indexName}
					<p class="text-xs text-amber-600">{nameError}</p>
				{/if}
			</div>

			<div class="flex flex-col gap-1.5">
				<Label for="gsi-hash">Hash key</Label>
				<div class="flex gap-2">
					<Input
						id="gsi-hash"
						bind:value={hashKey}
						placeholder="status"
						autocomplete="off"
						class="flex-1 font-mono"
					/>
					<Select
						type="single"
						value={hashKeyType}
						onValueChange={(v) => (hashKeyType = v as ScalarType)}
						disabled={hashTypeLocked}
					>
						<SelectTrigger aria-label="Hash key type" class="w-32">
							{SCALAR_TYPE_LABELS[hashKeyType]}
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="S" label="String">String</SelectItem>
							<SelectItem value="N" label="Number">Number</SelectItem>
							<SelectItem value="B" label="Binary">Binary</SelectItem>
						</SelectContent>
					</Select>
				</div>
				{#if hashTypeLocked}
					<p class="text-[10px] text-muted-foreground">
						Type locked: this attribute is already defined on the table.
					</p>
				{/if}
			</div>

			<div class="flex flex-col gap-1.5">
				<Label for="gsi-range">Range key (optional)</Label>
				<div class="flex gap-2">
					<Input
						id="gsi-range"
						bind:value={rangeKey}
						placeholder="leave blank if none"
						autocomplete="off"
						class="flex-1 font-mono"
					/>
					<Select
						type="single"
						value={rangeKeyType}
						onValueChange={(v) => (rangeKeyType = v as ScalarType)}
						disabled={rangeTypeLocked}
					>
						<SelectTrigger aria-label="Range key type" class="w-32">
							{SCALAR_TYPE_LABELS[rangeKeyType]}
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="S" label="String">String</SelectItem>
							<SelectItem value="N" label="Number">Number</SelectItem>
							<SelectItem value="B" label="Binary">Binary</SelectItem>
						</SelectContent>
					</Select>
				</div>
				{#if rangeError}
					<p class="text-xs text-amber-600">{rangeError}</p>
				{:else if rangeTypeLocked}
					<p class="text-[10px] text-muted-foreground">
						Type locked: this attribute is already defined on the table.
					</p>
				{/if}
			</div>

			<div class="flex flex-col gap-1.5">
				<Label for="gsi-projection">Projection</Label>
				<Select
					type="single"
					value={projectionType}
					onValueChange={(v) => (projectionType = v as ProjectionType)}
				>
					<SelectTrigger id="gsi-projection" class="w-full">
						{PROJECTION_LABELS[projectionType]}
					</SelectTrigger>
					<SelectContent>
						<SelectItem value="KEYS_ONLY" label={PROJECTION_LABELS.KEYS_ONLY}>
							{PROJECTION_LABELS.KEYS_ONLY}
						</SelectItem>
						<SelectItem value="INCLUDE" label={PROJECTION_LABELS.INCLUDE}>
							{PROJECTION_LABELS.INCLUDE}
						</SelectItem>
						<SelectItem value="ALL" label={PROJECTION_LABELS.ALL}>
							{PROJECTION_LABELS.ALL}
						</SelectItem>
					</SelectContent>
				</Select>
			</div>

			{#if projectionType === 'INCLUDE'}
				<div class="flex flex-col gap-1.5">
					<Label for="gsi-include">Non-key attributes</Label>
					<Textarea
						id="gsi-include"
						bind:value={includeAttrsText}
						placeholder="email, displayName, lastSeen"
						rows={2}
						class="font-mono text-xs"
					/>
					<p class="text-[10px] text-muted-foreground">
						Comma- or whitespace-separated. These ride along on every query so the consumer
						doesn't need a follow-up GetItem.
					</p>
				</div>
			{/if}

			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{/if}

			<DialogFooter>
				<Button type="button" variant="outline" onclick={onClose} disabled={saving}>
					Cancel
				</Button>
				<Button type="submit" disabled={!canSubmit}>
					{#if saving}
						<Loader2 class="size-3.5 animate-spin" />
					{/if}
					Create index
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
