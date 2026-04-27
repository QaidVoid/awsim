<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toast } from 'svelte-sonner';
	import { createTargetGroup } from '$lib/api/elb';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let protocol = $state<'HTTP' | 'HTTPS' | 'TCP' | 'TLS' | 'UDP'>('HTTP');
	let port = $state(80);
	let targetType = $state<'instance' | 'ip' | 'lambda' | 'alb'>('instance');
	let vpcId = $state('');
	let creating = $state(false);

	function reset() {
		name = '';
		protocol = 'HTTP';
		port = 80;
		targetType = 'instance';
		vpcId = '';
	}

	async function submit() {
		if (!name.trim()) {
			toast.error('Name is required.');
			return;
		}
		creating = true;
		try {
			await createTargetGroup({
				name: name.trim(),
				protocol,
				port,
				targetType,
				vpcId: vpcId.trim() || undefined,
			});
			toast.success('Target group created.');
			reset();
			onOpenChange(false);
			onCreated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create target group');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New target group</DialogTitle>
			<DialogDescription>
				Routes traffic from listeners to a set of registered targets.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="tg-create-name">Name</Label>
				<Input id="tg-create-name" bind:value={name} placeholder="my-tg" autocomplete="off" />
			</div>
			<div class="grid grid-cols-3 gap-3">
				<div class="flex flex-col gap-1">
					<Label for="tg-create-protocol">Protocol</Label>
					<select
						id="tg-create-protocol"
						bind:value={protocol}
						class="border-input dark:bg-input/30 h-9 rounded-md border bg-transparent px-2 text-sm shadow-xs outline-none focus-visible:ring-3"
					>
						<option value="HTTP">HTTP</option>
						<option value="HTTPS">HTTPS</option>
						<option value="TCP">TCP</option>
						<option value="TLS">TLS</option>
						<option value="UDP">UDP</option>
					</select>
				</div>
				<div class="flex flex-col gap-1">
					<Label for="tg-create-port">Port</Label>
					<Input
						id="tg-create-port"
						type="number"
						min="1"
						max="65535"
						bind:value={port}
					/>
				</div>
				<div class="flex flex-col gap-1">
					<Label for="tg-create-type">Target type</Label>
					<select
						id="tg-create-type"
						bind:value={targetType}
						class="border-input dark:bg-input/30 h-9 rounded-md border bg-transparent px-2 text-sm shadow-xs outline-none focus-visible:ring-3"
					>
						<option value="instance">instance</option>
						<option value="ip">ip</option>
						<option value="lambda">lambda</option>
						<option value="alb">alb</option>
					</select>
				</div>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="tg-create-vpc">VPC ID (optional)</Label>
				<Input
					id="tg-create-vpc"
					bind:value={vpcId}
					placeholder="vpc-xxxxx"
					autocomplete="off"
				/>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim()}>
				{creating ? 'Creating…' : 'Create'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
