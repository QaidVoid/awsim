<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listInstanceProfiles,
		createInstanceProfile,
		deleteInstanceProfile,
		listRoles,
		addRoleToInstanceProfile,
		removeRoleFromInstanceProfile,
		type IamInstanceProfile,
		type IamRole
	} from '$lib/api/iam';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import EntityDetailSheet from './entity-detail-sheet.svelte';
	import Server from '@lucide/svelte/icons/server';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import X from '@lucide/svelte/icons/x';
	import { toast } from 'svelte-sonner';

	let profiles = $state<IamInstanceProfile[]>([]);
	let allRoles = $state<IamRole[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let selected = $state<IamInstanceProfile | null>(null);
	let detailLoading = $state(false);
	let createOpen = $state(false);
	let createName = $state('');
	let creating = $state(false);
	let deleting = $state(false);
	let rolePickerName = $state('');
	let addingRole = $state(false);

	const filtered = $derived(
		filter.trim()
			? profiles.filter((p) =>
					p.instanceProfileName.toLowerCase().includes(filter.trim().toLowerCase())
				)
			: profiles
	);

	const availableRoles = $derived(
		allRoles.filter(
			(r) => !(selected?.roles ?? []).some((sr) => sr.roleName === r.roleName)
		)
	);

	async function load() {
		loading = true;
		try {
			profiles = await listInstanceProfiles();
		} finally {
			loading = false;
		}
	}

	async function loadRoles() {
		if (allRoles.length > 0) return;
		try {
			allRoles = await listRoles();
		} catch {
			/* picker stays empty */
		}
	}

	async function openDetail(p: IamInstanceProfile) {
		selected = p;
		detailLoading = true;
		try {
			await loadRoles();
			const fresh = await listInstanceProfiles();
			selected = fresh.find((ip) => ip.arn === p.arn) ?? p;
		} finally {
			detailLoading = false;
		}
	}

	async function handleCreate() {
		if (!createName.trim()) return;
		creating = true;
		try {
			await createInstanceProfile(createName.trim());
			toast.success(`Created ${createName.trim()}`);
			createName = '';
			createOpen = false;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Create failed');
		} finally {
			creating = false;
		}
	}

	async function handleDelete(p: IamInstanceProfile) {
		if (!confirm(`Delete instance profile "${p.instanceProfileName}"?`)) return;
		deleting = true;
		try {
			await deleteInstanceProfile(p.instanceProfileName);
			toast.success(`Deleted ${p.instanceProfileName}`);
			selected = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Delete failed');
		} finally {
			deleting = false;
		}
	}

	async function addRole() {
		if (!selected || !rolePickerName) return;
		addingRole = true;
		try {
			await addRoleToInstanceProfile(selected.instanceProfileName, rolePickerName);
			toast.success(`Attached role ${rolePickerName}`);
			rolePickerName = '';
			await load();
			const fresh = await listInstanceProfiles();
			selected = fresh.find((ip) => ip.arn === selected!.arn) ?? selected!;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to add role');
		} finally {
			addingRole = false;
		}
	}

	async function removeRole(roleName: string) {
		if (!selected) return;
		if (!confirm(`Remove role "${roleName}" from ${selected.instanceProfileName}?`)) return;
		try {
			await removeRoleFromInstanceProfile(selected.instanceProfileName, roleName);
			toast.success(`Removed role ${roleName}`);
			await load();
			const fresh = await listInstanceProfiles();
			selected = fresh.find((ip) => ip.arn === selected!.arn) ?? selected!;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to remove role');
		}
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-6 py-3">
		<Input
			type="search"
			placeholder="Filter instance profiles..."
			bind:value={filter}
			class="h-8 max-w-xs"
		/>
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {profiles.length}</Badge>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<Plus class="size-3.5" />
			<span class="ml-1">New instance profile</span>
		</Button>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-hidden">
		<DataTable
			rows={filtered}
			{loading}
			columns={[
				{ key: 'instanceProfileName', label: 'Name', width: '30%' },
				{ key: 'arn', label: 'ARN', mono: true }
			]}
			rowKey={(r: IamInstanceProfile) => r.arn}
			onRowClick={openDetail}
		>
			{#snippet empty()}
				<EmptyState
					icon={Server}
					title="No instance profiles"
					description="Create one with: aws iam create-instance-profile --instance-profile-name my-profile"
				/>
			{/snippet}
		</DataTable>
	</div>
</div>

<EntityDetailSheet
	open={!!selected}
	onOpenChange={(v) => {
		if (!v) selected = null;
	}}
	title={selected?.instanceProfileName ?? ''}
	subtitle={selected?.arn}
>
	{#if selected}
		<dl class="grid grid-cols-3 gap-x-4 gap-y-2 py-4 text-sm">
			<dt class="text-muted-foreground">Profile ID</dt>
			<dd class="col-span-2 font-mono text-xs">{selected.instanceProfileId}</dd>
			<dt class="text-muted-foreground">ARN</dt>
			<dd class="col-span-2 break-all font-mono text-xs">{selected.arn}</dd>
		</dl>

		<div class="mt-4">
			<div class="mb-2 flex items-center justify-between">
				<h3 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
					Role
				</h3>
				<Badge variant="outline">{selected.roles.length}/1</Badge>
			</div>
			{#if detailLoading}
				<p class="text-xs text-muted-foreground">Loading...</p>
			{:else if selected.roles.length === 0}
				<p class="text-xs text-muted-foreground">No role attached.</p>
			{:else}
				<ul class="space-y-1.5">
					{#each selected.roles as r (r.roleName)}
						<li
							class="flex items-center gap-2 rounded border border-border/60 px-3 py-2 text-sm"
						>
							<div class="min-w-0 flex-1">
								<div class="font-medium">{r.roleName}</div>
								<div class="truncate font-mono text-xs text-muted-foreground">{r.arn}</div>
							</div>
							<Button
								variant="ghost"
								size="icon-sm"
								aria-label="Remove role"
								onclick={() => removeRole(r.roleName)}
							>
								<X class="size-3.5" />
							</Button>
						</li>
					{/each}
				</ul>
			{/if}
			{#if selected.roles.length === 0}
				<div class="mt-2 flex gap-2">
					<select
						bind:value={rolePickerName}
						class="h-8 flex-1 rounded-md border border-border bg-background px-2 text-xs disabled:opacity-50"
						disabled={availableRoles.length === 0}
					>
						<option value="">
							{availableRoles.length === 0 ? '(no roles available)' : 'Select a role…'}
						</option>
						{#each availableRoles as r (r.roleName)}
							<option value={r.roleName}>{r.roleName}</option>
						{/each}
					</select>
					<Button size="sm" onclick={addRole} disabled={!rolePickerName || addingRole}>
						<Plus class="size-3.5" />
						<span class="ml-1">Attach</span>
					</Button>
				</div>
			{/if}
		</div>

		<div class="flex justify-end pt-4">
			<Button
				variant="destructive"
				size="sm"
				disabled={deleting}
				onclick={() => selected && handleDelete(selected)}
			>
				<Trash2 class="size-4" />
				<span class="ml-1">Delete profile</span>
			</Button>
		</div>
	{/if}
</EntityDetailSheet>

{#if createOpen}
	<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
		<div class="w-full max-w-sm rounded-lg border border-border bg-background p-6 shadow-lg">
			<h2 class="text-lg font-semibold">New instance profile</h2>
			<div class="mt-4">
				<Input
					type="text"
					placeholder="Instance profile name"
					bind:value={createName}
					onkeydown={(e) => e.key === 'Enter' && handleCreate()}
				/>
			</div>
			<div class="mt-4 flex justify-end gap-2">
				<Button variant="outline" size="sm" onclick={() => (createOpen = false)}>Cancel</Button>
				<Button size="sm" onclick={handleCreate} disabled={creating || !createName.trim()}>
					{creating ? 'Creating…' : 'Create'}
				</Button>
			</div>
		</div>
	</div>
{/if}
