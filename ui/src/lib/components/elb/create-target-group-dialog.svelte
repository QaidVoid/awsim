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
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
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
					<Select
						type="single"
						value={protocol}
						onValueChange={(v) =>
							(protocol = v as 'HTTP' | 'HTTPS' | 'TCP' | 'TLS' | 'UDP')}
					>
						<SelectTrigger id="tg-create-protocol" class="w-full">
							{protocol}
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="HTTP" label="HTTP">HTTP</SelectItem>
							<SelectItem value="HTTPS" label="HTTPS">HTTPS</SelectItem>
							<SelectItem value="TCP" label="TCP">TCP</SelectItem>
							<SelectItem value="TLS" label="TLS">TLS</SelectItem>
							<SelectItem value="UDP" label="UDP">UDP</SelectItem>
						</SelectContent>
					</Select>
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
					<Select
						type="single"
						value={targetType}
						onValueChange={(v) =>
							(targetType = v as 'instance' | 'ip' | 'lambda' | 'alb')}
					>
						<SelectTrigger id="tg-create-type" class="w-full">
							{targetType}
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="instance" label="instance">instance</SelectItem>
							<SelectItem value="ip" label="ip">ip</SelectItem>
							<SelectItem value="lambda" label="lambda">lambda</SelectItem>
							<SelectItem value="alb" label="alb">alb</SelectItem>
						</SelectContent>
					</Select>
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
