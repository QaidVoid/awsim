<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { addCustomAttributes, type SchemaAttribute, type UserPoolDetail } from '$lib/api/cognito';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuItem,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import Plus from '@lucide/svelte/icons/plus';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';

	interface Props {
		pool: UserPoolDetail | null;
		onRefresh: () => void | Promise<void>;
	}

	let { pool, onRefresh }: Props = $props();

	const TYPE_OPTIONS = ['String', 'Number', 'DateTime', 'Boolean'] as const;

	let standardAttrs = $derived<SchemaAttribute[]>(
		(pool?.schemaAttributes ?? []).filter((a) => !a.name.startsWith('custom:'))
	);
	let customAttrs = $derived<SchemaAttribute[]>(
		(pool?.schemaAttributes ?? []).filter((a) => a.name.startsWith('custom:'))
	);

	let addOpen = $state(false);
	let saving = $state(false);
	let error = $state<string | null>(null);
	let refreshing = $state(false);

	// Form state for the add-attribute dialog.
	let attrName = $state('');
	let attrType = $state<(typeof TYPE_OPTIONS)[number]>('String');
	let attrMutable = $state(true);
	let attrRequired = $state(false);
	let minLen = $state('');
	let maxLen = $state('');
	let minVal = $state('');
	let maxVal = $state('');

	function reset() {
		attrName = '';
		attrType = 'String';
		attrMutable = true;
		attrRequired = false;
		minLen = '';
		maxLen = '';
		minVal = '';
		maxVal = '';
		error = null;
		saving = false;
	}

	$effect(() => {
		if (!addOpen) reset();
	});

	async function refresh() {
		refreshing = true;
		try {
			await onRefresh();
		} finally {
			refreshing = false;
		}
	}

	async function submit() {
		const trimmed = attrName.trim();
		if (!trimmed) {
			error = 'Name is required';
			return;
		}
		if (!/^[a-zA-Z0-9_]+$/.test(trimmed)) {
			error = 'Name must be alphanumeric or underscore (no spaces or punctuation)';
			return;
		}
		// `custom:` prefix is auto-applied server-side; reject if user typed it.
		if (trimmed.startsWith('custom:')) {
			error = 'Drop the "custom:" prefix - it is added automatically.';
			return;
		}

		const payload: Parameters<typeof addCustomAttributes>[1][number] = {
			name: trimmed,
			type: attrType,
			mutable: attrMutable,
			required: attrRequired
		};

		if (attrType === 'String') {
			const sc: { minLength?: number; maxLength?: number } = {};
			if (minLen.trim()) {
				const n = Number.parseInt(minLen, 10);
				if (!Number.isFinite(n) || n < 0) {
					error = 'MinLength must be a non-negative integer';
					return;
				}
				sc.minLength = n;
			}
			if (maxLen.trim()) {
				const n = Number.parseInt(maxLen, 10);
				if (!Number.isFinite(n) || n < 0) {
					error = 'MaxLength must be a non-negative integer';
					return;
				}
				sc.maxLength = n;
			}
			if (
				sc.minLength !== undefined &&
				sc.maxLength !== undefined &&
				sc.minLength > sc.maxLength
			) {
				error = 'MinLength must be <= MaxLength';
				return;
			}
			if (Object.keys(sc).length > 0) payload.stringConstraints = sc;
		}

		if (attrType === 'Number') {
			const nc: { minValue?: number; maxValue?: number } = {};
			if (minVal.trim()) {
				const n = Number.parseInt(minVal, 10);
				if (!Number.isFinite(n)) {
					error = 'MinValue must be an integer';
					return;
				}
				nc.minValue = n;
			}
			if (maxVal.trim()) {
				const n = Number.parseInt(maxVal, 10);
				if (!Number.isFinite(n)) {
					error = 'MaxValue must be an integer';
					return;
				}
				nc.maxValue = n;
			}
			if (
				nc.minValue !== undefined &&
				nc.maxValue !== undefined &&
				nc.minValue > nc.maxValue
			) {
				error = 'MinValue must be <= MaxValue';
				return;
			}
			if (Object.keys(nc).length > 0) payload.numberConstraints = nc;
		}

		saving = true;
		error = null;
		try {
			await addCustomAttributes(pool!.id, [payload]);
			toast.success(`Added custom:${trimmed}`, {
				description:
					'App clients using the AWS default (unrestricted) can use it right away. Clients with explicit attribute permissions need it granted on the App clients tab.'
			});
			addOpen = false;
			await onRefresh();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Add custom attribute failed';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}

	function constraintLabel(a: SchemaAttribute): string | null {
		if (a.stringConstraints) {
			const { minLength, maxLength } = a.stringConstraints;
			if (minLength !== undefined && maxLength !== undefined) return `${minLength}..${maxLength} chars`;
			if (maxLength !== undefined) return `<= ${maxLength} chars`;
			if (minLength !== undefined) return `>= ${minLength} chars`;
		}
		if (a.numberConstraints) {
			const { minValue, maxValue } = a.numberConstraints;
			if (minValue !== undefined && maxValue !== undefined) return `${minValue}..${maxValue}`;
			if (maxValue !== undefined) return `<= ${maxValue}`;
			if (minValue !== undefined) return `>= ${minValue}`;
		}
		return null;
	}
</script>

<div class="w-full space-y-4 overflow-y-auto px-6 py-4">
	<div class="flex items-center justify-between">
		<div>
			<h2 class="text-base font-semibold">Schema attributes</h2>
			<p class="text-xs text-muted-foreground">
				Attributes must be declared here before you can set them on a user. Standard OIDC
				attributes are seeded automatically; custom attributes can be added after pool
				creation.
			</p>
		</div>
		<div class="flex items-center gap-2">
			<Button
				variant="outline"
				size="sm"
				onclick={() => void refresh()}
				disabled={refreshing}
				title="Reload schema"
			>
				<RefreshCw class="size-3.5 {refreshing ? 'animate-spin' : ''}" />
			</Button>
			<Button size="sm" onclick={() => (addOpen = true)} disabled={!pool}>
				<Plus class="size-3.5" />
				Add custom attribute
			</Button>
		</div>
	</div>

	<section class="space-y-2">
		<h3 class="text-xs font-medium uppercase tracking-wide text-muted-foreground">
			Custom ({customAttrs.length}/50)
		</h3>
		{#if customAttrs.length === 0}
			<p class="rounded border border-dashed border-border p-3 text-xs text-muted-foreground">
				No custom attributes yet. Add one above to use it on user records.
			</p>
		{:else}
			<div class="overflow-hidden rounded border border-border">
				<table class="w-full text-sm">
					<thead class="bg-muted/40 text-xs uppercase tracking-wide text-muted-foreground">
						<tr>
							<th class="px-3 py-2 text-left font-medium">Name</th>
							<th class="px-3 py-2 text-left font-medium">Type</th>
							<th class="px-3 py-2 text-left font-medium">Constraints</th>
							<th class="px-3 py-2 text-left font-medium">Required</th>
							<th class="px-3 py-2 text-left font-medium">Mutable</th>
						</tr>
					</thead>
					<tbody>
						{#each customAttrs as a (a.name)}
							<tr class="border-t border-border">
								<td class="px-3 py-2 font-mono text-xs">{a.name}</td>
								<td class="px-3 py-2">{a.type}</td>
								<td class="px-3 py-2 text-xs text-muted-foreground"
									>{constraintLabel(a) ?? '—'}</td
								>
								<td class="px-3 py-2">
									{#if a.required}<Badge variant="secondary">required</Badge>{:else}<span
											class="text-xs text-muted-foreground">no</span
										>{/if}
								</td>
								<td class="px-3 py-2">
									{#if a.mutable}<span class="text-xs text-muted-foreground">yes</span
										>{:else}<Badge variant="secondary">immutable</Badge>{/if}
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</section>

	<section class="space-y-2">
		<h3 class="text-xs font-medium uppercase tracking-wide text-muted-foreground">
			Standard ({standardAttrs.length})
		</h3>
		<div class="overflow-hidden rounded border border-border">
			<table class="w-full text-sm">
				<thead class="bg-muted/40 text-xs uppercase tracking-wide text-muted-foreground">
					<tr>
						<th class="px-3 py-2 text-left font-medium">Name</th>
						<th class="px-3 py-2 text-left font-medium">Type</th>
						<th class="px-3 py-2 text-left font-medium">Required</th>
						<th class="px-3 py-2 text-left font-medium">Mutable</th>
					</tr>
				</thead>
				<tbody>
					{#each standardAttrs as a (a.name)}
						<tr class="border-t border-border">
							<td class="px-3 py-2 font-mono text-xs">{a.name}</td>
							<td class="px-3 py-2">{a.type}</td>
							<td class="px-3 py-2"
								>{#if a.required}<Badge variant="secondary">required</Badge>{:else}<span
										class="text-xs text-muted-foreground">no</span
									>{/if}</td
							>
							<td class="px-3 py-2"
								>{#if a.mutable}<span class="text-xs text-muted-foreground">yes</span
									>{:else}<Badge variant="secondary">immutable</Badge>{/if}</td
							>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</section>
</div>

<Dialog bind:open={addOpen} onOpenChange={(v: boolean) => !v && (addOpen = false)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Add custom attribute</DialogTitle>
			<DialogDescription>
				Defines a new <code>custom:</code>-prefixed attribute on the schema. Once added, an
				attribute cannot be removed or have its type changed.
			</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="flex flex-col gap-1.5">
				<Label for="attr-name">Name</Label>
				<div class="flex items-stretch gap-1">
					<span
						class="inline-flex items-center rounded-l border border-r-0 border-input bg-muted px-2 text-xs text-muted-foreground"
					>
						custom:
					</span>
					<Input
						id="attr-name"
						bind:value={attrName}
						placeholder="plan"
						class="rounded-l-none"
						autocomplete="off"
					/>
				</div>
			</div>

			<div class="flex flex-col gap-1.5">
				<Label for="attr-type">Type</Label>
				<DropdownMenu>
					<DropdownMenuTrigger>
						<Button
							id="attr-type"
							variant="outline"
							class="w-full justify-between font-normal"
							type="button"
						>
							{attrType}
							<ChevronDown class="size-3.5 opacity-60" />
						</Button>
					</DropdownMenuTrigger>
					<DropdownMenuContent align="start" class="w-[var(--radix-dropdown-menu-trigger-width)]">
						{#each TYPE_OPTIONS as t (t)}
							<DropdownMenuItem onclick={() => (attrType = t)}>{t}</DropdownMenuItem>
						{/each}
					</DropdownMenuContent>
				</DropdownMenu>
			</div>

			{#if attrType === 'String'}
				<div class="grid grid-cols-2 gap-2">
					<div class="flex flex-col gap-1.5">
						<Label for="min-len" class="text-xs">MinLength (optional)</Label>
						<Input id="min-len" type="number" bind:value={minLen} min="0" placeholder="0" />
					</div>
					<div class="flex flex-col gap-1.5">
						<Label for="max-len" class="text-xs">MaxLength (optional)</Label>
						<Input id="max-len" type="number" bind:value={maxLen} min="0" placeholder="2048" />
					</div>
				</div>
			{/if}

			{#if attrType === 'Number'}
				<div class="grid grid-cols-2 gap-2">
					<div class="flex flex-col gap-1.5">
						<Label for="min-val" class="text-xs">MinValue (optional)</Label>
						<Input id="min-val" type="number" bind:value={minVal} placeholder="0" />
					</div>
					<div class="flex flex-col gap-1.5">
						<Label for="max-val" class="text-xs">MaxValue (optional)</Label>
						<Input id="max-val" type="number" bind:value={maxVal} placeholder="100" />
					</div>
				</div>
			{/if}

			<label class="flex items-center gap-2 text-sm">
				<input type="checkbox" bind:checked={attrMutable} class="size-3.5" />
				Mutable (uncheck to freeze the value once set)
			</label>
			<label class="flex items-center gap-2 text-sm">
				<input type="checkbox" bind:checked={attrRequired} class="size-3.5" />
				Required at user creation
			</label>

			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{/if}
			<DialogFooter>
				<Button type="button" variant="outline" onclick={() => (addOpen = false)} disabled={saving}>
					Cancel
				</Button>
				<Button type="submit" disabled={saving}>
					{#if saving}<Loader2 class="size-3.5 animate-spin" />{/if}
					Add
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
