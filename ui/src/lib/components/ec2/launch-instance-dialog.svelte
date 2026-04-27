<script lang="ts">
	import {
		runInstances,
		type KeyPair,
		type SecurityGroup,
		type Subnet
	} from '$lib/api/ec2';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter
	} from '$lib/components/ui/dialog';
	import { toast } from 'svelte-sonner';
	import RocketIcon from '@lucide/svelte/icons/rocket';

	interface Props {
		open: boolean;
		subnets: Subnet[];
		keys: KeyPair[];
		groups: SecurityGroup[];
		onOpenChange: (open: boolean) => void;
		onLaunched: () => void;
	}

	let { open, subnets, keys, groups, onOpenChange, onLaunched }: Props = $props();

	let name = $state('');
	let imageId = $state('ami-0abcdef1234567890');
	let instanceType = $state('t3.micro');
	let count = $state(1);
	let keyName = $state('');
	let subnetId = $state('');
	let securityGroupId = $state('');
	let launching = $state(false);

	function reset() {
		name = '';
		imageId = 'ami-0abcdef1234567890';
		instanceType = 't3.micro';
		count = 1;
		keyName = '';
		subnetId = '';
		securityGroupId = '';
	}

	async function handleSubmit(e: Event) {
		e.preventDefault();
		if (!imageId.trim() || !instanceType.trim()) return;
		launching = true;
		try {
			const ids = await runInstances({
				imageId: imageId.trim(),
				instanceType: instanceType.trim(),
				minCount: count,
				maxCount: count,
				keyName: keyName || undefined,
				subnetId: subnetId || undefined,
				securityGroupIds: securityGroupId ? [securityGroupId] : undefined,
				name: name.trim() || undefined
			});
			toast.success(`Launched ${ids.length} instance(s)`);
			reset();
			onOpenChange(false);
			onLaunched();
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Launch failed');
		} finally {
			launching = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>Launch instance</DialogTitle>
			<DialogDescription>Provision a new EC2 instance from an AMI.</DialogDescription>
		</DialogHeader>
		<form onsubmit={handleSubmit} class="grid grid-cols-2 gap-4 py-2">
			<div class="col-span-2 flex flex-col gap-1.5">
				<Label for="li-name">Name (tag)</Label>
				<Input id="li-name" bind:value={name} placeholder="my-instance" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="li-ami">AMI ID</Label>
				<Input id="li-ami" bind:value={imageId} placeholder="ami-..." required />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="li-type">Instance type</Label>
				<Input id="li-type" bind:value={instanceType} placeholder="t3.micro" required />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="li-count">Count</Label>
				<Input id="li-count" type="number" bind:value={count} min="1" max="10" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="li-key">Key pair</Label>
				<select
					id="li-key"
					bind:value={keyName}
					class="h-9 rounded-md border border-input bg-background px-3 py-1 text-sm shadow-xs"
				>
					<option value="">— None —</option>
					{#each keys as k (k.keyName)}
						<option value={k.keyName}>{k.keyName}</option>
					{/each}
				</select>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="li-subnet">Subnet</Label>
				<select
					id="li-subnet"
					bind:value={subnetId}
					class="h-9 rounded-md border border-input bg-background px-3 py-1 text-sm shadow-xs"
				>
					<option value="">— Default —</option>
					{#each subnets as s (s.subnetId)}
						<option value={s.subnetId}>{s.subnetId} ({s.availabilityZone})</option>
					{/each}
				</select>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="li-sg">Security group</Label>
				<select
					id="li-sg"
					bind:value={securityGroupId}
					class="h-9 rounded-md border border-input bg-background px-3 py-1 text-sm shadow-xs"
				>
					<option value="">— Default —</option>
					{#each groups as g (g.groupId)}
						<option value={g.groupId}>{g.groupName} ({g.groupId})</option>
					{/each}
				</select>
			</div>
			<DialogFooter class="col-span-2">
				<Button type="button" variant="ghost" onclick={() => onOpenChange(false)}>Cancel</Button>
				<Button type="submit" disabled={launching || !imageId.trim() || !instanceType.trim()}>
					<RocketIcon />
					{launching ? 'Launching...' : 'Launch'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
