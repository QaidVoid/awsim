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
	import { Switch } from '$lib/components/ui/switch';
	import { toast } from 'svelte-sonner';
	import { createHostedZone } from '$lib/api/route53';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let comment = $state('');
	let privateZone = $state(false);
	let creating = $state(false);

	function reset() {
		name = '';
		comment = '';
		privateZone = false;
	}

	async function submit() {
		if (!name.trim()) {
			toast.error('Zone name is required.');
			return;
		}
		creating = true;
		try {
			await createHostedZone({
				name: name.trim(),
				comment: comment.trim() || undefined,
				privateZone,
			});
			toast.success('Hosted zone created.');
			reset();
			onOpenChange(false);
			onCreated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create zone');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New hosted zone</DialogTitle>
			<DialogDescription>
				Hosted zones contain records mapping names to resources.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="r53-zone-name">Domain name</Label>
				<Input
					id="r53-zone-name"
					bind:value={name}
					placeholder="example.com"
					autocomplete="off"
				/>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="r53-zone-comment">Comment (optional)</Label>
				<Input id="r53-zone-comment" bind:value={comment} autocomplete="off" />
			</div>
			<div class="flex items-center justify-between rounded-md border border-border px-3 py-2">
				<div>
					<Label for="r53-zone-private" class="text-sm">Private zone</Label>
					<p class="text-[11px] text-muted-foreground">
						Resolves only inside associated VPCs.
					</p>
				</div>
				<Switch id="r53-zone-private" bind:checked={privateZone} />
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim()}>
				{creating ? 'Creating…' : 'Create zone'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
