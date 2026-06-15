<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { createDBCluster } from '$lib/api/rds';
	import { validateRdsDbIdentifier } from '$lib/validators';
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
		onCreated: (id: string) => void;
	}

	let { open = $bindable(false), onClose, onCreated }: Props = $props();

	let identifier = $state('');
	let engine = $state('aurora-postgresql');
	let masterUsername = $state('admin');
	let masterUserPassword = $state('changeme123');
	let serverless = $state(false);
	let minCapacity = $state(0.5);
	let maxCapacity = $state(16);
	let saving = $state(false);
	let error = $state<string | null>(null);

	const identifierError = $derived(
		identifier.trim() ? validateRdsDbIdentifier(identifier.trim()) : null
	);

	const ENGINE_LABELS: Record<string, string> = {
		'aurora-postgresql': 'Aurora PostgreSQL',
		'aurora-mysql': 'Aurora MySQL'
	};
	let engineLabel = $derived(ENGINE_LABELS[engine] ?? engine);

	$effect(() => {
		if (!open) {
			identifier = '';
			error = null;
			saving = false;
			serverless = false;
		}
	});

	async function submit() {
		const id = identifier.trim();
		if (!id) {
			error = 'Identifier required';
			return;
		}
		if (identifierError) {
			error = identifierError;
			return;
		}
		if (serverless && minCapacity > maxCapacity) {
			error = 'Min capacity must not exceed max capacity';
			return;
		}
		saving = true;
		error = null;
		try {
			await createDBCluster({
				identifier: id,
				engine,
				masterUsername,
				masterUserPassword,
				serverlessMinCapacity: serverless ? minCapacity : undefined,
				serverlessMaxCapacity: serverless ? maxCapacity : undefined
			});
			toast.success(`Created cluster ${id}`);
			onCreated(id);
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Failed to create cluster';
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
			<DialogTitle>Create Aurora cluster</DialogTitle>
			<DialogDescription>
				Provision a cluster, then add instances to it.
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
				<Label for="cluster-id">Identifier</Label>
				<Input
					id="cluster-id"
					bind:value={identifier}
					placeholder="my-aurora-cluster"
					autocomplete="off"
					aria-invalid={identifierError ? 'true' : undefined}
				/>
				{#if identifierError}
					<p class="text-[11px] text-destructive">{identifierError}</p>
				{:else}
					<p class="text-[11px] text-muted-foreground">
						1-63 chars, starts with a letter, letters / digits / hyphens.
					</p>
				{/if}
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="cluster-engine">Engine</Label>
				<Select type="single" bind:value={engine}>
					<SelectTrigger id="cluster-engine" class="w-full">
						{engineLabel}
					</SelectTrigger>
					<SelectContent>
						<SelectItem value="aurora-postgresql" label="Aurora PostgreSQL">
							Aurora PostgreSQL
						</SelectItem>
						<SelectItem value="aurora-mysql" label="Aurora MySQL">Aurora MySQL</SelectItem>
					</SelectContent>
				</Select>
			</div>
			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1.5">
					<Label for="cluster-user">Master user</Label>
					<Input id="cluster-user" bind:value={masterUsername} />
				</div>
				<div class="flex flex-col gap-1.5">
					<Label for="cluster-pass">Master password</Label>
					<Input
						id="cluster-pass"
						type="password"
						bind:value={masterUserPassword}
						autocomplete="new-password"
					/>
				</div>
			</div>
			<label class="flex items-center gap-2 text-xs">
				<input type="checkbox" bind:checked={serverless} class="size-3.5" />
				Serverless v2 scaling
			</label>
			{#if serverless}
				<div class="grid grid-cols-2 gap-3">
					<div class="flex flex-col gap-1.5">
						<Label for="cluster-min">Min ACU</Label>
						<Input id="cluster-min" type="number" bind:value={minCapacity} min={0.5} step={0.5} />
					</div>
					<div class="flex flex-col gap-1.5">
						<Label for="cluster-max">Max ACU</Label>
						<Input id="cluster-max" type="number" bind:value={maxCapacity} min={0.5} step={0.5} />
					</div>
				</div>
			{/if}
			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{/if}
			<DialogFooter>
				<Button type="button" variant="outline" onclick={onClose} disabled={saving}>
					Cancel
				</Button>
				<Button
					type="submit"
					disabled={saving || !identifier.trim() || identifierError !== null}
				>
					{#if saving}
						<Loader2 class="size-3.5 animate-spin" />
					{/if}
					Create
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
