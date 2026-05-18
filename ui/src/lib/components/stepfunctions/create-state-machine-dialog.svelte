<script lang="ts">
	import { createStateMachine } from '$lib/api/stepfunctions';
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
	import Plus from '@lucide/svelte/icons/plus';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	const DEFAULT_DEF = JSON.stringify(
		{
			Comment: 'A simple state machine',
			StartAt: 'HelloWorld',
			States: {
				HelloWorld: {
					Type: 'Pass',
					Result: 'Hello, World!',
					End: true
				}
			}
		},
		null,
		2
	);

	let name = $state('');
	let type = $state<'STANDARD' | 'EXPRESS'>('STANDARD');
	let definition = $state(DEFAULT_DEF);
	let creating = $state(false);

	const typeLabel = $derived(type === 'EXPRESS' ? 'Express' : 'Standard');

	async function submit(e: Event) {
		e.preventDefault();
		if (!name.trim()) return;
		creating = true;
		try {
			await createStateMachine({
				name: name.trim(),
				definition: definition.trim(),
				type
			});
			toast.success(`Created ${name.trim()}`);
			onCreated();
			onOpenChange(false);
			name = '';
			definition = DEFAULT_DEF;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Create failed');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-2xl">
		<DialogHeader>
			<DialogTitle>Create state machine</DialogTitle>
			<DialogDescription>
				Define an Amazon States Language workflow.
			</DialogDescription>
		</DialogHeader>
		<form onsubmit={submit} class="grid grid-cols-2 gap-3 py-2">
			<div class="flex flex-col gap-1.5">
				<Label for="sm-name">Name</Label>
				<Input id="sm-name" bind:value={name} placeholder="my-machine" required />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="sm-type">Type</Label>
				<Select
					type="single"
					value={type}
					onValueChange={(v) => (type = v as 'STANDARD' | 'EXPRESS')}
				>
					<SelectTrigger id="sm-type" class="w-full">
						{typeLabel}
					</SelectTrigger>
					<SelectContent>
						<SelectItem value="STANDARD" label="Standard">Standard</SelectItem>
						<SelectItem value="EXPRESS" label="Express">Express</SelectItem>
					</SelectContent>
				</Select>
			</div>
			<div class="col-span-2 flex flex-col gap-1.5">
				<Label for="sm-def">ASL definition</Label>
				<textarea
					id="sm-def"
					bind:value={definition}
					rows="14"
					spellcheck="false"
					class="resize-y rounded-md border border-border bg-background p-3 font-mono text-xs outline-none focus:border-ring focus:ring-1 focus:ring-ring"
				></textarea>
			</div>
			<DialogFooter class="col-span-2">
				<Button type="button" variant="ghost" onclick={() => onOpenChange(false)}>
					Cancel
				</Button>
				<Button type="submit" disabled={creating || !name.trim()}>
					<Plus />
					{creating ? 'Creating...' : 'Create state machine'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
