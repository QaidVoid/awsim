<script lang="ts">
	import {
		updateFunctionConfiguration,
		type LambdaConfiguration,
		type UpdateConfigurationInput
	} from '$lib/api/lambda';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { toast } from 'svelte-sonner';
	import Save from '@lucide/svelte/icons/save';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';

	interface Props {
		config: LambdaConfiguration;
		runtimes: string[];
		onSaved: (next: LambdaConfiguration) => void;
	}

	let { config, runtimes, onSaved }: Props = $props();

	let runtime = $state('');
	let handler = $state('');
	let memorySize = $state(128);
	let timeoutSec = $state(3);
	let description = $state('');
	let envEntries = $state<{ key: string; value: string }[]>([]);
	let saving = $state(false);
	let lastConfigName = $state('');

	$effect(() => {
		if (config.name !== lastConfigName) {
			lastConfigName = config.name;
			runtime = config.runtime;
			handler = config.handler;
			memorySize = config.memorySize;
			timeoutSec = config.timeout;
			description = config.description;
			envEntries = Object.entries(config.envVars).map(([key, value]) => ({
				key,
				value
			}));
		}
	});

	function addEnv() {
		envEntries = [...envEntries, { key: '', value: '' }];
	}

	function removeEnv(idx: number) {
		envEntries = envEntries.filter((_, i) => i !== idx);
	}

	async function handleSave(e: Event) {
		e.preventDefault();
		saving = true;
		try {
			const envVars: Record<string, string> = {};
			for (const e of envEntries) {
				if (e.key.trim()) envVars[e.key.trim()] = e.value;
			}
			const input: UpdateConfigurationInput = {
				runtime,
				handler,
				memorySize: Number(memorySize),
				timeout: Number(timeoutSec),
				description,
				envVars
			};
			const next = await updateFunctionConfiguration(config.name, input);
			toast.success('Configuration saved');
			onSaved(next);
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Save failed');
		} finally {
			saving = false;
		}
	}
</script>

<form onsubmit={handleSave} class="flex flex-col gap-4 p-4">
	<section class="rounded-md border border-border bg-card">
		<header class="border-b border-border px-4 py-3">
			<h3 class="text-sm font-medium">Runtime settings</h3>
		</header>
		<div class="grid grid-cols-1 gap-4 px-4 py-3 sm:grid-cols-2">
			<div class="flex flex-col gap-1.5">
				<Label for="cfg-runtime">Runtime</Label>
				<Select type="single" bind:value={runtime}>
					<SelectTrigger id="cfg-runtime" class="w-full">
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
				<Label for="cfg-handler">Handler</Label>
				<Input id="cfg-handler" bind:value={handler} placeholder="index.handler" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="cfg-memory">Memory (MB)</Label>
				<Input
					id="cfg-memory"
					type="number"
					min="128"
					max="10240"
					bind:value={memorySize}
				/>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="cfg-timeout">Timeout (s)</Label>
				<Input
					id="cfg-timeout"
					type="number"
					min="1"
					max="900"
					bind:value={timeoutSec}
				/>
			</div>
			<div class="flex flex-col gap-1.5 sm:col-span-2">
				<Label for="cfg-desc">Description</Label>
				<Input id="cfg-desc" bind:value={description} />
			</div>
		</div>
	</section>

	<section class="rounded-md border border-border bg-card">
		<header class="flex items-center justify-between border-b border-border px-4 py-3">
			<div>
				<h3 class="text-sm font-medium">Environment variables</h3>
				<p class="mt-0.5 text-xs text-muted-foreground">
					Plain text key/value pairs available to your function via env.
				</p>
			</div>
			<Button type="button" variant="outline" size="sm" onclick={addEnv}>
				<Plus />
				Add
			</Button>
		</header>
		<div class="flex flex-col gap-2 px-4 py-3">
			{#if envEntries.length === 0}
				<p class="py-3 text-center text-xs text-muted-foreground">No variables.</p>
			{:else}
				{#each envEntries as entry, idx (idx)}
					<div class="flex items-center gap-2">
						<Input
							placeholder="KEY"
							bind:value={entry.key}
							class="font-mono text-xs"
						/>
						<Input
							placeholder="value"
							bind:value={entry.value}
							class="font-mono text-xs"
						/>
						<Button
							type="button"
							variant="ghost"
							size="icon-sm"
							onclick={() => removeEnv(idx)}
							aria-label="Remove variable"
						>
							<Trash2 />
						</Button>
					</div>
				{/each}
			{/if}
		</div>
	</section>

	<div class="flex justify-end">
		<Button type="submit" disabled={saving}>
			<Save />
			{saving ? 'Saving...' : 'Save changes'}
		</Button>
	</div>
</form>
