<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import FileCogIcon from '@lucide/svelte/icons/file-cog';
	import { toast } from 'svelte-sonner';
	import {
		listProfiles,
		createProfile,
		deleteProfile,
		createHostedVersion,
		type ConfigProfile
	} from '$lib/api/appconfig';

	interface Props {
		appId: string;
		refreshKey?: number;
	}

	let { appId, refreshKey = 0 }: Props = $props();

	let rows = $state<ConfigProfile[]>([]);
	let loading = $state(false);
	let newName = $state('');
	let creating = $state(false);

	let pubProfile = $state<string>('');
	let pubContent = $state('{\n  "feature_x": true\n}');
	let publishing = $state(false);

	$effect(() => {
		appId;
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			rows = await listProfiles(appId);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load profiles');
		} finally {
			loading = false;
		}
	}

	async function create() {
		if (!newName.trim()) return toast.error('Profile name is required.');
		creating = true;
		try {
			const p = await createProfile(appId, newName.trim());
			toast.success(`Created profile "${p.name}".`);
			newName = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create profile');
		} finally {
			creating = false;
		}
	}

	async function remove(p: ConfigProfile) {
		if (!confirm(`Delete profile "${p.name}"? Hosted versions are removed too.`)) return;
		try {
			await deleteProfile(appId, p.id);
			toast.success('Profile deleted.');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}

	async function publishVersion() {
		if (!pubProfile) return toast.error('Pick a profile.');
		publishing = true;
		try {
			const b64 = btoa(pubContent);
			const v = await createHostedVersion({
				appId,
				profileId: pubProfile,
				contentBase64: b64
			});
			toast.success(`Published version ${v.versionNumber}.`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to publish version');
		} finally {
			publishing = false;
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Configuration profiles
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="flex items-center gap-2">
		<Input bind:value={newName} placeholder="profile name" class="h-8 max-w-[260px]" />
		<Button size="sm" onclick={create} disabled={creating}>
			<PlusIcon />
			{creating ? 'Creating…' : 'Create profile'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'id', label: 'ID', mono: true, width: '120px' },
			{ key: 'type', label: 'Type', width: '160px', cell: typeCell },
			{ key: 'locationUri', label: 'Location', mono: true },
			{ key: '__actions', label: '', width: '60px', cell: actionsCell }
		]}
		rowKey={(r) => r.id}
	>
		{#snippet empty()}
			<EmptyState
				icon={FileCogIcon}
				title="No profiles"
				description="Create a profile to hold configuration content (feature flags, JSON, etc.)."
			/>
		{/snippet}
	</DataTable>

	{#if rows.length > 0}
		<div class="space-y-2 rounded-md border border-border p-3">
			<div class="text-xs font-semibold">Publish hosted configuration version</div>
			<div class="flex items-center gap-2">
				<select
					bind:value={pubProfile}
					class="h-8 flex-1 rounded-md border border-border bg-background px-2 text-xs"
				>
					<option value="">Pick profile…</option>
					{#each rows as p (p.id)}
						<option value={p.id}>{p.name}</option>
					{/each}
				</select>
				<Button size="sm" onclick={publishVersion} disabled={publishing}>
					{publishing ? 'Publishing…' : 'Publish version'}
				</Button>
			</div>
			<Textarea bind:value={pubContent} rows={6} class="font-mono text-xs" />
		</div>
	{/if}
</div>

{#snippet typeCell(row: ConfigProfile)}
	<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">{row.type}</Badge>
{/snippet}

{#snippet actionsCell(row: ConfigProfile)}
	<Button variant="ghost" size="xs" onclick={() => remove(row)}>
		<Trash2Icon class="text-destructive" />
	</Button>
{/snippet}
