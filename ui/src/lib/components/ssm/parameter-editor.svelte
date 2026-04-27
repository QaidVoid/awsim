<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle,
		SheetFooter
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import EyeOffIcon from '@lucide/svelte/icons/eye-off';
	import { toast } from 'svelte-sonner';
	import {
		getParameter,
		putParameter,
		type ParameterType,
		type ParameterValue
	} from '$lib/api/ssm';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		paramName: string | null;
		onSaved: () => void;
	}

	let { open = $bindable(), onOpenChange, paramName, onSaved }: Props = $props();

	let loading = $state(false);
	let saving = $state(false);
	let name = $state('');
	let value = $state('');
	let type = $state<ParameterType>('String');
	let description = $state('');
	let revealed = $state(false);
	let detail = $state<ParameterValue | null>(null);

	const isNew = $derived(paramName === null);

	$effect(() => {
		if (open) load();
	});

	async function load() {
		detail = null;
		revealed = false;
		if (!paramName) {
			name = '';
			value = '';
			type = 'String';
			description = '';
			return;
		}
		loading = true;
		try {
			const p = await getParameter(paramName);
			detail = p;
			name = p.name;
			value = p.value;
			type = p.type;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load parameter');
		} finally {
			loading = false;
		}
	}

	async function save() {
		if (!name.trim()) {
			toast.error('Name is required.');
			return;
		}
		if (!value.trim() && type !== 'StringList') {
			toast.error('Value is required.');
			return;
		}
		saving = true;
		try {
			await putParameter({
				name: name.trim(),
				value,
				type,
				description: description.trim() || undefined,
				overwrite: true
			});
			toast.success(isNew ? 'Parameter created.' : 'Parameter updated.');
			onSaved();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save');
		} finally {
			saving = false;
		}
	}

	const masked = $derived(value ? '•'.repeat(Math.min(value.length, 32)) : '');
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-xl overflow-y-auto sm:max-w-xl">
		<SheetHeader>
			<SheetTitle>{isNew ? 'Put parameter' : 'Edit parameter'}</SheetTitle>
			<SheetDescription>
				{#if detail}
					Version <Badge variant="outline" class="ml-1 h-4 px-1.5 text-[10px]"
						>v{detail.version}</Badge
					>
					{#if detail.lastModifiedDate}
						<span class="ml-2 text-xs">last modified {detail.lastModifiedDate}</span>
					{/if}
				{:else}
					Stores configuration values, secrets, and feature flags hierarchically.
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex flex-col gap-3 px-6 pb-6">
			{#if loading}
				<div class="flex flex-col gap-3">
					<Skeleton class="h-9 w-full" />
					<Skeleton class="h-9 w-1/3" />
					<Skeleton class="h-24 w-full" />
				</div>
			{:else}
				<div class="flex flex-col gap-1">
					<Label for="ssm-name">Name</Label>
					<Input
						id="ssm-name"
						bind:value={name}
						placeholder="/app/prod/db-url"
						readonly={!isNew}
						class="font-mono text-xs"
					/>
				</div>

				<div class="flex flex-col gap-1">
					<Label for="ssm-type">Type</Label>
					<select
						id="ssm-type"
						bind:value={type}
						class="border-input bg-background h-9 rounded-md border px-2 text-sm"
					>
						<option value="String">String</option>
						<option value="StringList">StringList</option>
						<option value="SecureString">SecureString</option>
					</select>
					{#if type === 'StringList'}
						<p class="text-[11px] text-muted-foreground">
							Provide values as a comma-separated list.
						</p>
					{:else if type === 'SecureString'}
						<p class="text-[11px] text-muted-foreground">
							Encrypted at rest with the default KMS key.
						</p>
					{/if}
				</div>

				<div class="flex flex-col gap-1">
					<div class="flex items-center justify-between">
						<Label for="ssm-value">Value</Label>
						{#if type === 'SecureString'}
							<Button variant="ghost" size="xs" onclick={() => (revealed = !revealed)}>
								{#if revealed}<EyeOffIcon class="size-3" /> Hide{:else}<EyeIcon class="size-3" /> Reveal{/if}
							</Button>
						{/if}
					</div>
					<Textarea
						id="ssm-value"
						value={type === 'SecureString' && !revealed ? masked : value}
						readonly={type === 'SecureString' && !revealed}
						oninput={(e) => {
							if (type !== 'SecureString' || revealed) {
								value = (e.currentTarget as HTMLTextAreaElement).value;
							}
						}}
						rows={5}
						class="font-mono text-xs"
					/>
				</div>

				<div class="flex flex-col gap-1">
					<Label for="ssm-description">Description (optional)</Label>
					<Input id="ssm-description" bind:value={description} />
				</div>
			{/if}
		</div>

		<SheetFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={save} disabled={saving || loading || !name.trim()}>
				{saving ? 'Saving…' : isNew ? 'Create parameter' : 'Save new version'}
			</Button>
		</SheetFooter>
	</SheetContent>
</Sheet>
