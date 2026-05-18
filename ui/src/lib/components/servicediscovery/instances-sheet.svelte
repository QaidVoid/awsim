<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { toast } from 'svelte-sonner';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import {
		listInstances,
		registerInstance,
		deregisterInstance,
		type Instance,
		type SDService
	} from '$lib/api/servicediscovery';

	interface Props {
		open: boolean;
		service: SDService | null;
		onOpenChange: (open: boolean) => void;
		onChanged?: () => void;
	}

	let { open, service, onOpenChange, onChanged }: Props = $props();

	let instances = $state<Instance[]>([]);
	let loading = $state(false);
	let newId = $state('');
	let newIp = $state('');
	let newPort = $state('');
	let busy = $state(false);
	let deleteTarget = $state<Instance | null>(null);
	let deleteOpen = $state(false);
	let deleteBusy = $state(false);

	$effect(() => {
		if (open && service) {
			void load(service.id);
		} else if (!open) {
			instances = [];
		}
	});

	async function load(serviceId: string) {
		loading = true;
		try {
			instances = await listInstances(serviceId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load instances');
		} finally {
			loading = false;
		}
	}

	async function register() {
		if (!service) return;
		if (!newId.trim()) return toast.error('Instance ID is required.');
		const attrs: Record<string, string> = {};
		if (newIp.trim()) attrs.AWS_INSTANCE_IPV4 = newIp.trim();
		if (newPort.trim()) attrs.AWS_INSTANCE_PORT = newPort.trim();
		busy = true;
		try {
			await registerInstance(service.id, newId.trim(), attrs);
			toast.success('Instance registered.');
			newId = '';
			newIp = '';
			newPort = '';
			await load(service.id);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to register instance');
		} finally {
			busy = false;
		}
	}

	function remove(inst: Instance) {
		if (!service) return;
		deleteTarget = inst;
		deleteOpen = true;
	}

	async function confirmRemove() {
		if (!service || !deleteTarget) return;
		deleteBusy = true;
		try {
			await deregisterInstance(service.id, deleteTarget.id);
			toast.success('Instance deregistered.');
			deleteOpen = false;
			deleteTarget = null;
			await load(service.id);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to deregister');
		} finally {
			deleteBusy = false;
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-xl">
		<SheetHeader>
			<SheetTitle>Service instances</SheetTitle>
			<SheetDescription>
				{#if service}
					<span class="font-mono text-xs">{service.name} • {service.id}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4">
			<div class="space-y-2 rounded-md border border-border p-3">
				<div class="text-xs font-semibold">Register instance</div>
				<div class="grid grid-cols-3 gap-2">
					<Input bind:value={newId} placeholder="instance ID" class="h-8 col-span-3 font-mono text-xs" />
					<Input bind:value={newIp} placeholder="10.0.0.5" class="h-8 col-span-2 font-mono text-xs" />
					<Input bind:value={newPort} placeholder="8080" class="h-8 font-mono text-xs" />
				</div>
				<Button size="sm" onclick={register} disabled={busy}>
					<PlusIcon />
					{busy ? 'Registering…' : 'Register'}
				</Button>
			</div>

			{#if loading}
				<p class="text-xs text-muted-foreground">Loading…</p>
			{:else if instances.length === 0}
				<div class="rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground">
					No instances registered.
				</div>
			{:else}
				<div class="space-y-1.5">
					{#each instances as inst (inst.id)}
						<div class="flex items-start justify-between rounded-md border border-border p-2 text-xs">
							<div class="flex flex-col gap-0.5">
								<span class="font-mono">{inst.id}</span>
								<div class="flex flex-wrap gap-1">
									{#each Object.entries(inst.attributes) as [k, v] (k)}
										<span class="font-mono text-[10px] text-muted-foreground">
											{k}={v}
										</span>
									{/each}
								</div>
							</div>
							<Button variant="ghost" size="xs" onclick={() => remove(inst)}>
								<Trash2Icon class="text-destructive" />
							</Button>
						</div>
					{/each}
				</div>
			{/if}
		</div>
	</SheetContent>
</Sheet>

<ConfirmDialog
	bind:open={deleteOpen}
	title="Deregister instance?"
	description={`Deregister instance "${deleteTarget?.id ?? ''}".`}
	confirmLabel="Deregister"
	busy={deleteBusy}
	onConfirm={confirmRemove}
	onClose={() => {
		deleteOpen = false;
		deleteTarget = null;
	}}
/>
