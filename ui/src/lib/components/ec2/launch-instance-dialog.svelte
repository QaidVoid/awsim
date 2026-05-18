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
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
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

	let subnetLabel = $derived.by(() => {
		const s = subnets.find((x) => x.subnetId === subnetId);
		return s ? `${s.subnetId} (${s.availabilityZone})` : '— Default —';
	});
	let securityGroupLabel = $derived.by(() => {
		const g = groups.find((x) => x.groupId === securityGroupId);
		return g ? `${g.groupName} (${g.groupId})` : '— Default —';
	});

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
				<Select type="single" bind:value={keyName}>
					<SelectTrigger id="li-key" class="w-full">
						{keyName || '— None —'}
					</SelectTrigger>
					<SelectContent>
						<SelectItem value="" label="— None —">— None —</SelectItem>
						{#each keys as k (k.keyName)}
							<SelectItem value={k.keyName} label={k.keyName}>{k.keyName}</SelectItem>
						{/each}
					</SelectContent>
				</Select>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="li-subnet">Subnet</Label>
				<Select type="single" bind:value={subnetId}>
					<SelectTrigger id="li-subnet" class="w-full">
						{subnetLabel}
					</SelectTrigger>
					<SelectContent>
						<SelectItem value="" label="— Default —">— Default —</SelectItem>
						{#each subnets as s (s.subnetId)}
							<SelectItem
								value={s.subnetId}
								label={`${s.subnetId} (${s.availabilityZone})`}
								>{s.subnetId} ({s.availabilityZone})</SelectItem
							>
						{/each}
					</SelectContent>
				</Select>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="li-sg">Security group</Label>
				<Select type="single" bind:value={securityGroupId}>
					<SelectTrigger id="li-sg" class="w-full">
						{securityGroupLabel}
					</SelectTrigger>
					<SelectContent>
						<SelectItem value="" label="— Default —">— Default —</SelectItem>
						{#each groups as g (g.groupId)}
							<SelectItem
								value={g.groupId}
								label={`${g.groupName} (${g.groupId})`}
								>{g.groupName} ({g.groupId})</SelectItem
							>
						{/each}
					</SelectContent>
				</Select>
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
