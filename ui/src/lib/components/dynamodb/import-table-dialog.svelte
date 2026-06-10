<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { importTableFromS3, type ScalarType } from '$lib/api/dynamodb';
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
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		open: boolean;
		onClose: () => void;
		onImported: (name: string) => void;
	}

	let { open = $bindable(false), onClose, onImported }: Props = $props();

	const SCALAR_TYPE_LABELS: Record<ScalarType, string> = {
		S: 'String',
		N: 'Number',
		B: 'Binary'
	};

	let tableName = $state('');
	let pkName = $state('');
	let pkType = $state<ScalarType>('S');
	let skName = $state('');
	let skType = $state<ScalarType>('S');
	let bucket = $state('');
	let keyPrefix = $state('');
	let inputFormat = $state<'DYNAMODB_JSON' | 'CSV'>('DYNAMODB_JSON');
	let compression = $state<'NONE' | 'GZIP'>('NONE');
	let importing = $state(false);

	$effect(() => {
		if (!open) {
			tableName = '';
			pkName = '';
			pkType = 'S';
			skName = '';
			skType = 'S';
			bucket = '';
			keyPrefix = '';
			inputFormat = 'DYNAMODB_JSON';
			compression = 'NONE';
			importing = false;
		}
	});

	const valid = $derived(!!tableName.trim() && !!pkName.trim() && !!bucket.trim());

	async function runImport() {
		if (!valid) return;
		importing = true;
		try {
			const result = await importTableFromS3({
				tableName: tableName.trim(),
				partitionKey: pkName.trim(),
				partitionKeyType: pkType,
				sortKey: skName.trim() || undefined,
				sortKeyType: skName.trim() ? skType : undefined,
				bucket: bucket.trim(),
				keyPrefix: keyPrefix.trim() || undefined,
				inputFormat,
				compression
			});
			if (result.status === 'FAILED' || result.failureCode) {
				toast.error(result.failureMessage ?? 'Import failed');
			} else {
				const errors = result.errorCount > 0 ? `, ${result.errorCount} skipped` : '';
				toast.success(
					`Imported ${result.importedCount.toLocaleString()} item${result.importedCount === 1 ? '' : 's'} into ${tableName.trim()}${errors}`
				);
			}
			open = false;
			onImported(tableName.trim());
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Import failed');
		} finally {
			importing = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(v) => (v ? undefined : onClose())}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>Import table from S3</DialogTitle>
			<DialogDescription>
				Creates a new table and loads every object under the bucket and prefix. Use
				DynamoDB JSON (one <code>{'{"Item": ...}'}</code> per line, what exports produce) or
				CSV with a header row.
			</DialogDescription>
		</DialogHeader>

		<div class="space-y-3 py-1">
			<div class="space-y-1.5">
				<Label for="ddb-import-name">New table name</Label>
				<Input id="ddb-import-name" bind:value={tableName} class="font-mono" placeholder="orders" />
			</div>

			<div class="grid grid-cols-[1fr_140px] gap-2">
				<div class="space-y-1.5">
					<Label for="ddb-import-pk">Partition key</Label>
					<Input id="ddb-import-pk" bind:value={pkName} class="font-mono" placeholder="pk" />
				</div>
				<div class="space-y-1.5">
					<Label>Type</Label>
					<Select type="single" value={pkType} onValueChange={(v) => (pkType = v as ScalarType)}>
						<SelectTrigger class="text-xs">{SCALAR_TYPE_LABELS[pkType]}</SelectTrigger>
						<SelectContent>
							{#each Object.entries(SCALAR_TYPE_LABELS) as [v, label] (v)}
								<SelectItem value={v} {label}>{label}</SelectItem>
							{/each}
						</SelectContent>
					</Select>
				</div>
			</div>

			<div class="grid grid-cols-[1fr_140px] gap-2">
				<div class="space-y-1.5">
					<Label for="ddb-import-sk">Sort key (optional)</Label>
					<Input id="ddb-import-sk" bind:value={skName} class="font-mono" placeholder="sk" />
				</div>
				<div class="space-y-1.5">
					<Label>Type</Label>
					<Select type="single" value={skType} onValueChange={(v) => (skType = v as ScalarType)}>
						<SelectTrigger class="text-xs" disabled={!skName.trim()}>
							{SCALAR_TYPE_LABELS[skType]}
						</SelectTrigger>
						<SelectContent>
							{#each Object.entries(SCALAR_TYPE_LABELS) as [v, label] (v)}
								<SelectItem value={v} {label}>{label}</SelectItem>
							{/each}
						</SelectContent>
					</Select>
				</div>
			</div>

			<div class="grid grid-cols-2 gap-2">
				<div class="space-y-1.5">
					<Label for="ddb-import-bucket">S3 bucket</Label>
					<Input id="ddb-import-bucket" bind:value={bucket} class="font-mono" placeholder="my-bucket" />
				</div>
				<div class="space-y-1.5">
					<Label for="ddb-import-prefix">Key prefix (optional)</Label>
					<Input id="ddb-import-prefix" bind:value={keyPrefix} class="font-mono" placeholder="exports/" />
				</div>
			</div>

			<div class="grid grid-cols-2 gap-2">
				<div class="space-y-1.5">
					<Label>Input format</Label>
					<Select
						type="single"
						value={inputFormat}
						onValueChange={(v) => (inputFormat = v as 'DYNAMODB_JSON' | 'CSV')}
					>
						<SelectTrigger class="text-xs">{inputFormat}</SelectTrigger>
						<SelectContent>
							<SelectItem value="DYNAMODB_JSON" label="DYNAMODB_JSON">DYNAMODB_JSON</SelectItem>
							<SelectItem value="CSV" label="CSV">CSV</SelectItem>
						</SelectContent>
					</Select>
				</div>
				<div class="space-y-1.5">
					<Label>Compression</Label>
					<Select
						type="single"
						value={compression}
						onValueChange={(v) => (compression = v as 'NONE' | 'GZIP')}
					>
						<SelectTrigger class="text-xs">{compression}</SelectTrigger>
						<SelectContent>
							<SelectItem value="NONE" label="NONE">NONE</SelectItem>
							<SelectItem value="GZIP" label="GZIP">GZIP</SelectItem>
						</SelectContent>
					</Select>
				</div>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => (open = false)} disabled={importing}>
				Cancel
			</Button>
			<Button onclick={runImport} disabled={importing || !valid}>
				{#if importing}
					<Loader2 class="size-3.5 animate-spin" />
				{/if}
				<span class="ml-1">{importing ? 'Importing…' : 'Import'}</span>
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
