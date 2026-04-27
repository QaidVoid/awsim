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
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
	import { toast } from 'svelte-sonner';
	import { createGuardrail } from '$lib/api/bedrock';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated?: (id: string) => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let description = $state('');
	let blockedInput = $state('This input is not allowed.');
	let blockedOutput = $state('This output is not allowed.');
	let creating = $state(false);

	function reset() {
		name = '';
		description = '';
		blockedInput = 'This input is not allowed.';
		blockedOutput = 'This output is not allowed.';
	}

	async function submit() {
		if (!name.trim()) {
			toast.error('Name is required.');
			return;
		}
		creating = true;
		try {
			const res = await createGuardrail({
				name: name.trim(),
				description: description.trim() || undefined,
				blockedInputMessaging: blockedInput,
				blockedOutputsMessaging: blockedOutput,
			});
			toast.success('Guardrail created.');
			reset();
			onOpenChange(false);
			onCreated?.(res.guardrailId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create guardrail');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New guardrail</DialogTitle>
			<DialogDescription>
				Define content policies that filter unsafe inputs and outputs for foundation models.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="bedrock-guardrail-name">Name</Label>
				<Input
					id="bedrock-guardrail-name"
					bind:value={name}
					placeholder="my-guardrail"
					autocomplete="off"
				/>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="bedrock-guardrail-desc">Description (optional)</Label>
				<Input id="bedrock-guardrail-desc" bind:value={description} autocomplete="off" />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="bedrock-guardrail-input">Blocked input message</Label>
				<Textarea id="bedrock-guardrail-input" bind:value={blockedInput} rows={2} />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="bedrock-guardrail-output">Blocked output message</Label>
				<Textarea id="bedrock-guardrail-output" bind:value={blockedOutput} rows={2} />
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim()}>
				{creating ? 'Creating…' : 'Create guardrail'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
