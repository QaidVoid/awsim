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
	import { createLoadBalancer } from '$lib/api/elb';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let type = $state<'application' | 'network' | 'gateway'>('application');
	let scheme = $state<'internet-facing' | 'internal'>('internet-facing');
	let subnets = $state('');
	let creating = $state(false);

	function reset() {
		name = '';
		type = 'application';
		scheme = 'internet-facing';
		subnets = '';
	}

	async function submit() {
		if (!name.trim()) {
			toast.error('Name is required.');
			return;
		}
		creating = true;
		try {
			await createLoadBalancer({
				name: name.trim(),
				type,
				scheme,
				subnetIds: subnets
					.split(/[\s,]+/)
					.map((s) => s.trim())
					.filter(Boolean),
			});
			toast.success('Load balancer created.');
			reset();
			onOpenChange(false);
			onCreated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create load balancer');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New load balancer</DialogTitle>
			<DialogDescription>
				Application, network, or gateway load balancer fronted by listeners.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="lb-create-name">Name</Label>
				<Input id="lb-create-name" bind:value={name} placeholder="my-alb" autocomplete="off" />
			</div>
			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1">
					<Label for="lb-create-type">Type</Label>
					<select
						id="lb-create-type"
						bind:value={type}
						class="border-input dark:bg-input/30 h-9 rounded-md border bg-transparent px-2 text-sm shadow-xs outline-none focus-visible:ring-3"
					>
						<option value="application">application</option>
						<option value="network">network</option>
						<option value="gateway">gateway</option>
					</select>
				</div>
				<div class="flex flex-col gap-1">
					<Label for="lb-create-scheme">Scheme</Label>
					<select
						id="lb-create-scheme"
						bind:value={scheme}
						class="border-input dark:bg-input/30 h-9 rounded-md border bg-transparent px-2 text-sm shadow-xs outline-none focus-visible:ring-3"
					>
						<option value="internet-facing">internet-facing</option>
						<option value="internal">internal</option>
					</select>
				</div>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="lb-create-subnets">Subnets (comma or space separated)</Label>
				<Input
					id="lb-create-subnets"
					bind:value={subnets}
					placeholder="subnet-aaaa, subnet-bbbb"
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
