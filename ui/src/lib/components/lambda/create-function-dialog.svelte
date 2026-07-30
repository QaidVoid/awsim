<script lang="ts">
	import { createFunction } from '$lib/api/lambda';
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
		runtimes: string[];
		onOpenChange: (open: boolean) => void;
		onCreated: (name: string) => void;
	}

	let { open, runtimes, onOpenChange, onCreated }: Props = $props();

	let name = $state('');
	let runtime = $state('python3.11');
	let handler = $state('index.handler');
	let role = $state('arn:aws:iam::000000000000:role/exec');
	let memorySize = $state(128);
	let timeoutSec = $state(3);
	let zipBase64 = $state('');
	let description = $state('');
	let architecture = $state('x86_64');
	let ephemeralStorageMb = $state(512);
	let envText = $state('');
	let tagsText = $state('');
	let creating = $state(false);

	$effect(() => {
		if (runtimes.length > 0 && !runtimes.includes(runtime)) {
			runtime = runtimes[0];
		}
	});

	function reset() {
		name = '';
		handler = 'index.handler';
		role = 'arn:aws:iam::000000000000:role/exec';
		memorySize = 128;
		timeoutSec = 3;
		zipBase64 = '';
		description = '';
		architecture = 'x86_64';
		ephemeralStorageMb = 512;
		envText = '';
		tagsText = '';
	}

	/** Parse `key=value` lines into a map, rejecting malformed rows. */
	function parsePairs(text: string): Record<string, string> | null {
		const out: Record<string, string> = {};
		for (const line of text.split('\n')) {
			const trimmed = line.trim();
			if (!trimmed) continue;
			const eq = trimmed.indexOf('=');
			if (eq <= 0) return null;
			out[trimmed.slice(0, eq).trim()] = trimmed.slice(eq + 1).trim();
		}
		return out;
	}

	async function submit(e: Event) {
		e.preventDefault();
		if (!name.trim()) return;
		const envVars = parsePairs(envText);
		if (envVars === null) {
			toast.error('Environment variables must be one `KEY=value` per line.');
			return;
		}
		const tags = parsePairs(tagsText);
		if (tags === null) {
			toast.error('Tags must be one `key=value` per line.');
			return;
		}
		creating = true;
		try {
			await createFunction({
				name: name.trim(),
				runtime,
				handler,
				role,
				memorySize: Number(memorySize),
				timeout: Number(timeoutSec),
				zipFileBase64: zipBase64.trim() || undefined,
				description: description.trim() || undefined,
				architecture,
				ephemeralStorageMb: Number(ephemeralStorageMb),
				envVars,
				tags
			});
			toast.success(`Created ${name.trim()}`);
			onCreated(name.trim());
			reset();
			onOpenChange(false);
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Create failed');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-lg max-h-[85vh] overflow-y-auto">
		<DialogHeader>
			<DialogTitle>Create Lambda function</DialogTitle>
			<DialogDescription>
				Provision a new function. ZIP contents are base64-encoded.
			</DialogDescription>
		</DialogHeader>
		<form onsubmit={submit} class="grid grid-cols-1 gap-3 py-2 sm:grid-cols-2">
			<div class="flex flex-col gap-1.5 sm:col-span-2">
				<Label for="cf-name">Function name</Label>
				<Input id="cf-name" bind:value={name} placeholder="my-function" required />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="cf-runtime">Runtime</Label>
				<Select type="single" bind:value={runtime}>
					<SelectTrigger id="cf-runtime" class="w-full">
						{runtime}
					</SelectTrigger>
					<SelectContent>
						{#each runtimes as rt (rt)}
							<SelectItem value={rt} label={rt}>{rt}</SelectItem>
						{/each}
					</SelectContent>
				</Select>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="cf-handler">Handler</Label>
				<Input id="cf-handler" bind:value={handler} />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="cf-memory">Memory (MB)</Label>
				<Input
					id="cf-memory"
					type="number"
					min="128"
					max="10240"
					bind:value={memorySize}
				/>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="cf-timeout">Timeout (s)</Label>
				<Input id="cf-timeout" type="number" min="1" max="900" bind:value={timeoutSec} />
			</div>
			<div class="flex flex-col gap-1.5 sm:col-span-2">
				<Label for="cf-role">Execution role ARN</Label>
				<Input id="cf-role" bind:value={role} class="font-mono text-xs" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="cf-arch">Architecture</Label>
				<select
					id="cf-arch"
					bind:value={architecture}
					class="h-9 rounded-md border border-border bg-background px-2 text-sm"
				>
					<option value="x86_64">x86_64</option>
					<option value="arm64">arm64 (Graviton)</option>
				</select>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="cf-ephemeral">Ephemeral storage (MB)</Label>
				<Input
					id="cf-ephemeral"
					type="number"
					min="512"
					max="10240"
					bind:value={ephemeralStorageMb}
				/>
			</div>
			<div class="flex flex-col gap-1.5 sm:col-span-2">
				<Label for="cf-description">Description (optional)</Label>
				<Input id="cf-description" bind:value={description} placeholder="What it does" />
			</div>
			<div class="flex flex-col gap-1.5 sm:col-span-2">
				<Label for="cf-zip">Code (base64 ZIP, optional)</Label>
				<Input
					id="cf-zip"
					bind:value={zipBase64}
					class="font-mono text-xs"
					placeholder="UEsDBBQAA..."
				/>
			</div>
			<div class="flex flex-col gap-1.5 sm:col-span-2">
				<Label for="cf-env">Environment variables</Label>
				<textarea
					id="cf-env"
					bind:value={envText}
					rows="3"
					placeholder="STAGE=dev&#10;TABLE_NAME=orders"
					class="rounded-md border border-border bg-background px-2 py-1.5 font-mono text-xs"
				></textarea>
				<p class="text-[11px] text-muted-foreground">
					One `KEY=value` per line. Available to the handler through the usual
					environment lookup.
				</p>
			</div>
			<div class="flex flex-col gap-1.5 sm:col-span-2">
				<Label for="cf-tags">Tags</Label>
				<textarea
					id="cf-tags"
					bind:value={tagsText}
					rows="2"
					placeholder="env=dev&#10;team=platform"
					class="rounded-md border border-border bg-background px-2 py-1.5 font-mono text-xs"
				></textarea>
			</div>
			<DialogFooter class="sm:col-span-2">
				<Button type="button" variant="ghost" onclick={() => onOpenChange(false)}>
					Cancel
				</Button>
				<Button type="submit" disabled={creating || !name.trim()}>
					<Plus />
					{creating ? 'Creating...' : 'Create function'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
