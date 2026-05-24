<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { createDBInstance } from '$lib/api/rds';
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
	let engine = $state('postgres');
	let instanceClass = $state('db.t3.micro');
	let allocatedStorage = $state(20);
	let masterUsername = $state('admin');
	let masterUserPassword = $state('changeme123');
	let saving = $state(false);
	let error = $state<string | null>(null);

	const identifierError = $derived(
		identifier.trim() ? validateRdsDbIdentifier(identifier.trim()) : null
	);

	const ENGINE_LABELS: Record<string, string> = {
		postgres: 'PostgreSQL',
		mysql: 'MySQL',
		mariadb: 'MariaDB'
	};
	let engineLabel = $derived(ENGINE_LABELS[engine] ?? engine);

	$effect(() => {
		if (!open) {
			identifier = '';
			error = null;
			saving = false;
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
		saving = true;
		error = null;
		try {
			await createDBInstance({
				identifier: id,
				engine,
				instanceClass,
				allocatedStorage,
				masterUsername,
				masterUserPassword
			});
			toast.success(`Created DB instance ${id}`);
			onCreated(id);
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Failed to create DB instance';
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
			<DialogTitle>Create DB instance</DialogTitle>
			<DialogDescription>Provision a relational database.</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="flex flex-col gap-1.5">
				<Label for="rds-id">Identifier</Label>
				<Input
					id="rds-id"
					bind:value={identifier}
					placeholder="my-database"
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
			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1.5">
					<Label for="rds-engine">Engine</Label>
					<Select type="single" bind:value={engine}>
						<SelectTrigger id="rds-engine" class="w-full">
							{engineLabel}
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="postgres" label="PostgreSQL">PostgreSQL</SelectItem>
							<SelectItem value="mysql" label="MySQL">MySQL</SelectItem>
							<SelectItem value="mariadb" label="MariaDB">MariaDB</SelectItem>
						</SelectContent>
					</Select>
				</div>
				<div class="flex flex-col gap-1.5">
					<Label for="rds-class">Instance class</Label>
					<Select type="single" bind:value={instanceClass}>
						<SelectTrigger id="rds-class" class="w-full">
							{instanceClass}
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="db.t3.micro" label="db.t3.micro">db.t3.micro</SelectItem>
							<SelectItem value="db.t3.small" label="db.t3.small">db.t3.small</SelectItem>
							<SelectItem value="db.t3.medium" label="db.t3.medium"
								>db.t3.medium</SelectItem
							>
							<SelectItem value="db.m5.large" label="db.m5.large">db.m5.large</SelectItem>
						</SelectContent>
					</Select>
				</div>
			</div>
			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1.5">
					<Label for="rds-storage">Storage (GiB)</Label>
					<Input id="rds-storage" type="number" bind:value={allocatedStorage} min={20} />
				</div>
				<div class="flex flex-col gap-1.5">
					<Label for="rds-user">Master user</Label>
					<Input id="rds-user" bind:value={masterUsername} />
				</div>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="rds-pass">Master password</Label>
				<Input
					id="rds-pass"
					type="password"
					bind:value={masterUserPassword}
					autocomplete="new-password"
				/>
			</div>
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
