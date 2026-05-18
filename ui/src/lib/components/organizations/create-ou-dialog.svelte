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
	import {
		Select,
		SelectContent,
		SelectItem,
		SelectTrigger
	} from '$lib/components/ui/select';
	import { toast } from 'svelte-sonner';
	import { createOrganizationalUnit, listRoots, type Root } from '$lib/api/organizations';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let parentId = $state('');
	let roots = $state<Root[]>([]);
	let creating = $state(false);

	$effect(() => {
		if (open && roots.length === 0) {
			listRoots()
				.then((r) => {
					roots = r.roots;
					if (!parentId && roots.length) parentId = roots[0].id;
				})
				.catch(() => {});
		}
	});

	function rootLabel(id: string): string {
		const r = roots.find((x) => x.id === id);
		return r ? `${r.name} (${r.id})` : id;
	}

	async function submit() {
		if (!name.trim()) {
			toast.error('OU name is required.');
			return;
		}
		if (!parentId) {
			toast.error('Pick a parent root.');
			return;
		}
		creating = true;
		try {
			await createOrganizationalUnit(parentId, name.trim());
			toast.success('Organizational unit created.');
			name = '';
			onOpenChange(false);
			onCreated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create OU');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New organizational unit</DialogTitle>
			<DialogDescription>
				An OU groups accounts under a root so SCPs can target them.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="ou-name">Name</Label>
				<Input id="ou-name" bind:value={name} placeholder="Workloads" autocomplete="off" />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="ou-parent">Parent root</Label>
				<Select type="single" bind:value={parentId}>
					<SelectTrigger id="ou-parent" class="w-full font-mono text-xs">
						{parentId ? rootLabel(parentId) : 'Select a root'}
					</SelectTrigger>
					<SelectContent>
						{#each roots as r (r.id)}
							<SelectItem value={r.id} label={`${r.name} (${r.id})`}>
								{r.name} ({r.id})
							</SelectItem>
						{/each}
					</SelectContent>
				</Select>
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim() || !parentId}>
				{creating ? 'Creating…' : 'Create OU'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
