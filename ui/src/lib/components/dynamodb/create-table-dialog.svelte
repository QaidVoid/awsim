<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { createTable, type ScalarType } from '$lib/api/dynamodb';
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
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		open: boolean;
		onClose: () => void;
		onCreated: (name: string) => void;
	}

	let { open = $bindable(false), onClose, onCreated }: Props = $props();

	const SCALAR_TYPE_LABELS: Record<ScalarType, string> = {
		S: 'String',
		N: 'Number',
		B: 'Binary'
	};

	let name = $state('');
	let pkName = $state('');
	let pkType = $state<ScalarType>('S');
	let skName = $state('');
	let skType = $state<ScalarType>('S');
	let deletionProtection = $state(false);
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) {
			name = '';
			pkName = '';
			pkType = 'S';
			skName = '';
			skType = 'S';
			deletionProtection = false;
			saving = false;
			error = null;
		}
	});

	async function submit() {
		if (!name.trim() || !pkName.trim()) {
			error = 'Table and partition key names are required';
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
				deletionProtectionEnabled: deletionProtection
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

<Dialog bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Create table</DialogTitle>
			<DialogDescription>Pay-per-request billing.</DialogDescription>
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
			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{/if}
			<DialogFooter>
				<Button type="button" variant="outline" onclick={onClose} disabled={saving}>
					Cancel
				</Button>
				<Button type="submit" disabled={saving || !name.trim() || !pkName.trim()}>
					{#if saving}
						<Loader2 class="size-3.5 animate-spin" />
					{/if}
					Create
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
