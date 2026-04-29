<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toast } from 'svelte-sonner';
	import { createBroker } from '$lib/api/mq';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let brokerName = $state('');
	let engineType = $state<'RABBITMQ' | 'ACTIVEMQ'>('RABBITMQ');
	let engineVersion = $state('3.13');
	let hostInstanceType = $state('mq.t3.micro');
	let deploymentMode = $state('SINGLE_INSTANCE');
	let initialUsername = $state('admin');
	let creating = $state(false);

	function reset() {
		brokerName = '';
		initialUsername = 'admin';
	}

	async function submit() {
		if (!brokerName.trim()) return toast.error('Broker name is required.');
		creating = true;
		try {
			await createBroker({
				brokerName: brokerName.trim(),
				engineType,
				engineVersion: engineVersion.trim(),
				hostInstanceType: hostInstanceType.trim(),
				deploymentMode,
				initialUser: initialUsername.trim()
					? { username: initialUsername.trim(), consoleAccess: true }
					: undefined
			});
			toast.success(`Created broker "${brokerName.trim()}".`);
			reset();
			onCreated?.();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create broker');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="max-w-md">
		<DialogHeader>
			<DialogTitle>Create MQ broker</DialogTitle>
			<DialogDescription>
				Create a new Amazon MQ broker. The broker is online immediately in AWSim — no real
				ActiveMQ/RabbitMQ process is started.
			</DialogDescription>
		</DialogHeader>

		<div class="space-y-3">
			<div class="space-y-1.5">
				<Label for="mq-name">Broker name</Label>
				<Input id="mq-name" bind:value={brokerName} placeholder="primary" />
			</div>

			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1.5">
					<Label for="mq-engine">Engine</Label>
					<select
						id="mq-engine"
						bind:value={engineType}
						class="h-9 w-full rounded-md border border-border bg-background px-3 text-sm"
					>
						<option value="RABBITMQ">RABBITMQ</option>
						<option value="ACTIVEMQ">ACTIVEMQ</option>
					</select>
				</div>
				<div class="space-y-1.5">
					<Label for="mq-ver">Engine version</Label>
					<Input id="mq-ver" bind:value={engineVersion} />
				</div>
			</div>

			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1.5">
					<Label for="mq-host">Host instance type</Label>
					<Input id="mq-host" bind:value={hostInstanceType} class="font-mono text-xs" />
				</div>
				<div class="space-y-1.5">
					<Label for="mq-dep">Deployment mode</Label>
					<select
						id="mq-dep"
						bind:value={deploymentMode}
						class="h-9 w-full rounded-md border border-border bg-background px-3 text-sm"
					>
						<option value="SINGLE_INSTANCE">SINGLE_INSTANCE</option>
						<option value="ACTIVE_STANDBY_MULTI_AZ">ACTIVE_STANDBY_MULTI_AZ</option>
						<option value="CLUSTER_MULTI_AZ">CLUSTER_MULTI_AZ</option>
					</select>
				</div>
			</div>

			<div class="space-y-1.5">
				<Label for="mq-user">Initial username <span class="text-muted-foreground">(optional)</span></Label>
				<Input id="mq-user" bind:value={initialUsername} placeholder="admin" />
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)} disabled={creating}>
				Cancel
			</Button>
			<Button onclick={submit} disabled={creating}>
				{creating ? 'Creating…' : 'Create broker'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
