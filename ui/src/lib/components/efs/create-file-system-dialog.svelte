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
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { toast } from 'svelte-sonner';
	import { createFileSystem } from '$lib/api/efs';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let creationToken = $state('');
	let performanceMode = $state('generalPurpose');
	let throughputMode = $state('bursting');
	let encrypted = $state(false);
	let creating = $state(false);

	function reset() {
		name = '';
		creationToken = '';
		performanceMode = 'generalPurpose';
		throughputMode = 'bursting';
		encrypted = false;
	}

	async function submit() {
		const token = creationToken.trim() || crypto.randomUUID();
		creating = true;
		try {
			await createFileSystem({
				creationToken: token,
				name: name.trim() || undefined,
				performanceMode,
				throughputMode,
				encrypted
			});
			toast.success(`Created file system${name ? ` "${name}"` : ''}.`);
			reset();
			onCreated?.();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create file system');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="max-w-md">
		<DialogHeader>
			<DialogTitle>New EFS file system</DialogTitle>
			<DialogDescription>
				Provision a new EFS file system. Mount targets and access points can be added afterwards.
			</DialogDescription>
		</DialogHeader>

		<div class="space-y-3">
			<div class="space-y-1.5">
				<Label for="efs-name">Name <span class="text-muted-foreground">(optional)</span></Label>
				<Input id="efs-name" bind:value={name} placeholder="data" />
			</div>
			<div class="space-y-1.5">
				<Label for="efs-token">Creation token <span class="text-muted-foreground">(optional, auto-generated)</span></Label>
				<Input id="efs-token" bind:value={creationToken} placeholder="auto" class="font-mono text-xs" />
			</div>
			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1.5">
					<Label for="efs-perf">Performance mode</Label>
					<Select type="single" bind:value={performanceMode}>
						<SelectTrigger id="efs-perf" class="w-full">
							{performanceMode}
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="generalPurpose" label="generalPurpose"
								>generalPurpose</SelectItem
							>
							<SelectItem value="maxIO" label="maxIO">maxIO</SelectItem>
						</SelectContent>
					</Select>
				</div>
				<div class="space-y-1.5">
					<Label for="efs-tput">Throughput mode</Label>
					<Select type="single" bind:value={throughputMode}>
						<SelectTrigger id="efs-tput" class="w-full">
							{throughputMode}
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="bursting" label="bursting">bursting</SelectItem>
							<SelectItem value="provisioned" label="provisioned"
								>provisioned</SelectItem
							>
							<SelectItem value="elastic" label="elastic">elastic</SelectItem>
						</SelectContent>
					</Select>
				</div>
			</div>
			<label class="flex items-center gap-2 text-sm">
				<input type="checkbox" bind:checked={encrypted} class="rounded border-border" />
				Encryption at rest
			</label>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)} disabled={creating}>
				Cancel
			</Button>
			<Button onclick={submit} disabled={creating}>
				{creating ? 'Creating…' : 'Create file system'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
