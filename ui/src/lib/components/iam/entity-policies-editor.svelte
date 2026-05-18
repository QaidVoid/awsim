<script lang="ts">
	import { onMount } from 'svelte';
	import { listPolicies, type IamAttachedPolicy, type IamPolicy } from '$lib/api/iam';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import PolicyEditor from './policy-editor.svelte';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Plus from '@lucide/svelte/icons/plus';
	import Edit3 from '@lucide/svelte/icons/edit-3';
	import Save from '@lucide/svelte/icons/save';
	import X from '@lucide/svelte/icons/x';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import { toast } from 'svelte-sonner';

	interface Props {
		// Currently attached managed policies + inline policy names.
		// The parent loads these and re-loads after each mutation; we
		// just render and dispatch back via the callbacks.
		attached: IamAttachedPolicy[];
		inlineNames: string[];
		// Hide the inline-policy section entirely. Useful for entities
		// the API client doesn't expose inline ops for (e.g. groups
		// in this UI today).
		showInline?: boolean;
		// Callbacks the parent provides to perform the actual API
		// calls (so this component can stay agnostic of whether it's
		// editing a user, role, or group).
		onAttach: (policyArn: string) => Promise<void>;
		onDetach: (policyArn: string) => Promise<void>;
		onLoadInline: (name: string) => Promise<string>;
		onPutInline: (name: string, document: string) => Promise<void>;
		onDeleteInline: (name: string) => Promise<void>;
		// Trigger parent reload after a successful mutation.
		onMutated: () => void;
	}

	let {
		attached,
		inlineNames,
		showInline = true,
		onAttach,
		onDetach,
		onLoadInline,
		onPutInline,
		onDeleteInline,
		onMutated,
	}: Props = $props();

	let allManaged = $state<IamPolicy[]>([]);
	let pickerArn = $state('');
	let attaching = $state(false);

	let editingName = $state<string | null>(null);
	let editingDoc = $state('');
	let editingNew = $state(false);
	let newInlineName = $state('');
	let savingInline = $state(false);

	let detachTarget = $state<{ arn: string; name: string } | null>(null);
	let detachOpen = $state(false);
	let detachBusy = $state(false);

	let deleteInlineTarget = $state<string | null>(null);
	let deleteInlineOpen = $state(false);
	let deleteInlineBusy = $state(false);

	const availableManaged = $derived(
		allManaged.filter((p) => !attached.some((a) => a.policyArn === p.arn))
	);

	onMount(async () => {
		try {
			allManaged = await listPolicies('Local');
		} catch {
			/* empty list is fine — picker just stays empty */
		}
	});

	async function handleAttach() {
		if (!pickerArn) return;
		attaching = true;
		try {
			await onAttach(pickerArn);
			toast.success('Policy attached');
			pickerArn = '';
			onMutated();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Attach failed');
		} finally {
			attaching = false;
		}
	}

	function handleDetach(arn: string, name: string) {
		detachTarget = { arn, name };
		detachOpen = true;
	}

	async function confirmDetach() {
		const t = detachTarget;
		if (!t) return;
		detachBusy = true;
		try {
			await onDetach(t.arn);
			toast.success(`Detached ${t.name}`);
			detachOpen = false;
			detachTarget = null;
			onMutated();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Detach failed');
		} finally {
			detachBusy = false;
		}
	}

	async function startEdit(name: string) {
		editingName = name;
		editingDoc = '';
		editingNew = false;
		try {
			const doc = await onLoadInline(name);
			try {
				editingDoc = JSON.stringify(JSON.parse(doc), null, 2);
			} catch {
				editingDoc = doc;
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Load failed');
			editingName = null;
		}
	}

	function startNewInline() {
		editingNew = true;
		editingName = '';
		newInlineName = '';
		editingDoc = JSON.stringify(
			{
				Version: '2012-10-17',
				Statement: [
					{
						Effect: 'Allow',
						Action: ['s3:GetObject'],
						Resource: ['arn:aws:s3:::example-bucket/*'],
					},
				],
			},
			null,
			2
		);
	}

	function cancelEdit() {
		editingName = null;
		editingNew = false;
		editingDoc = '';
		newInlineName = '';
	}

	async function saveInline() {
		const name = editingNew ? newInlineName.trim() : editingName;
		if (!name) {
			toast.error('Name is required');
			return;
		}
		try {
			JSON.parse(editingDoc);
		} catch {
			toast.error('Document is not valid JSON');
			return;
		}
		savingInline = true;
		try {
			await onPutInline(name, editingDoc);
			toast.success(`Saved ${name}`);
			cancelEdit();
			onMutated();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Save failed');
		} finally {
			savingInline = false;
		}
	}

	function handleDeleteInline(name: string) {
		deleteInlineTarget = name;
		deleteInlineOpen = true;
	}

	async function confirmDeleteInline() {
		const name = deleteInlineTarget;
		if (!name) return;
		deleteInlineBusy = true;
		try {
			await onDeleteInline(name);
			toast.success(`Deleted ${name}`);
			if (editingName === name) cancelEdit();
			deleteInlineOpen = false;
			deleteInlineTarget = null;
			onMutated();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleteInlineBusy = false;
		}
	}
</script>

<div class="space-y-6">
	<!-- Attached managed policies -->
	<section>
		<div class="mb-2 flex items-center justify-between">
			<h3 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
				Attached managed policies
			</h3>
			<Badge variant="outline">{attached.length}</Badge>
		</div>
		{#if attached.length === 0}
			<p class="text-xs text-muted-foreground">None attached.</p>
		{:else}
			<ul class="space-y-1">
				{#each attached as p (p.policyArn)}
					<li class="flex items-center gap-2 rounded border border-border/60 px-3 py-1.5">
						<div class="min-w-0 flex-1">
							<div class="text-sm font-medium">{p.policyName}</div>
							<div class="truncate font-mono text-[11px] text-muted-foreground">
								{p.policyArn}
							</div>
						</div>
						<Button
							variant="ghost"
							size="icon-sm"
							aria-label="Detach"
							onclick={() => handleDetach(p.policyArn, p.policyName)}
						>
							<X class="size-3.5" />
						</Button>
					</li>
				{/each}
			</ul>
		{/if}
		<div class="mt-2 flex gap-2">
			<select
				bind:value={pickerArn}
				class="h-8 flex-1 rounded-md border border-border bg-background px-2 text-xs disabled:opacity-50"
				disabled={availableManaged.length === 0}
			>
				<option value="">
					{availableManaged.length === 0
						? '(no other managed policies)'
						: 'Select a managed policy to attach…'}
				</option>
				{#each availableManaged as p (p.arn)}
					<option value={p.arn}>{p.policyName}</option>
				{/each}
			</select>
			<Button size="sm" onclick={handleAttach} disabled={!pickerArn || attaching}>
				<Plus class="size-3.5" />
				<span class="ml-1">Attach</span>
			</Button>
		</div>
	</section>

	<!-- Inline policies -->
	{#if showInline}
		<section>
		<div class="mb-2 flex items-center justify-between">
			<h3 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
				Inline policies
			</h3>
			<div class="flex items-center gap-2">
				<Badge variant="outline">{inlineNames.length}</Badge>
				<Button variant="outline" size="xs" onclick={startNewInline}>
					<Plus class="size-3" />
					<span class="ml-1">Add</span>
				</Button>
			</div>
		</div>
		{#if inlineNames.length === 0}
			<p class="text-xs text-muted-foreground">No inline policies.</p>
		{:else}
			<ul class="space-y-1">
				{#each inlineNames as name (name)}
					<li class="flex items-center gap-2 rounded border border-border/60 px-3 py-1.5">
						<span class="flex-1 truncate font-mono text-xs">{name}</span>
						<Button
							variant="ghost"
							size="icon-sm"
							aria-label="Edit"
							onclick={() => startEdit(name)}
						>
							<Edit3 class="size-3.5" />
						</Button>
						<Button
							variant="ghost"
							size="icon-sm"
							aria-label="Delete"
							onclick={() => handleDeleteInline(name)}
						>
							<Trash2 class="size-3.5" />
						</Button>
					</li>
				{/each}
			</ul>
		{/if}
		{#if editingName !== null || editingNew}
			<div class="mt-3 rounded border border-border bg-muted/20 p-3">
				<div class="mb-2 flex items-center gap-2">
					{#if editingNew}
						<Label for="new-inline-name" class="text-xs">Name</Label>
						<Input
							id="new-inline-name"
							bind:value={newInlineName}
							placeholder="my-policy"
							class="h-8 w-64 font-mono text-xs"
						/>
					{:else}
						<span class="font-mono text-xs">{editingName}</span>
					{/if}
					<div class="flex-1"></div>
					<Button variant="ghost" size="xs" onclick={cancelEdit}>Cancel</Button>
					<Button size="xs" onclick={saveInline} disabled={savingInline}>
						<Save class="size-3" />
						<span class="ml-1">{savingInline ? 'Saving…' : 'Save'}</span>
					</Button>
				</div>
				<PolicyEditor bind:value={editingDoc} id="inline-policy-edit" rows={12} />
			</div>
		{/if}
		</section>
	{/if}
</div>

<ConfirmDialog
	bind:open={detachOpen}
	title="Detach policy?"
	description={`Detach managed policy "${detachTarget?.name ?? ''}".`}
	confirmLabel="Detach"
	busy={detachBusy}
	onConfirm={confirmDetach}
	onClose={() => (detachOpen = false)}
/>

<ConfirmDialog
	bind:open={deleteInlineOpen}
	title="Delete inline policy?"
	description={`Delete inline policy "${deleteInlineTarget ?? ''}".`}
	busy={deleteInlineBusy}
	onConfirm={confirmDeleteInline}
	onClose={() => (deleteInlineOpen = false)}
/>
