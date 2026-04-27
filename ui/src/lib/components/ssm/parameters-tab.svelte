<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { DataTable, EmptyState } from '$lib/components/service';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import SettingsIcon from '@lucide/svelte/icons/settings-2';
	import { toast } from 'svelte-sonner';
	import {
		describeParameters,
		getParametersByPath,
		deleteParameter,
		type Parameter,
		type ParameterType
	} from '$lib/api/ssm';
	import ParameterEditor from './parameter-editor.svelte';

	let parameters = $state<Parameter[]>([]);
	let loading = $state(false);
	let pathFilter = $state('');
	let editorOpen = $state(false);
	let editingName = $state<string | null>(null);
	let confirmDelete = $state<string | null>(null);

	async function load() {
		loading = true;
		try {
			if (pathFilter.trim().startsWith('/')) {
				const values = await getParametersByPath(pathFilter.trim(), true, false);
				parameters = values.map((v) => ({
					name: v.name,
					type: v.type,
					version: v.version,
					lastModifiedDate: v.lastModifiedDate
				}));
			} else {
				const meta = await describeParameters();
				parameters = pathFilter.trim()
					? meta.filter((p) => p.name.toLowerCase().includes(pathFilter.trim().toLowerCase()))
					: meta;
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load parameters');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});

	function openCreate() {
		editingName = null;
		editorOpen = true;
	}

	function openEdit(name: string) {
		editingName = name;
		editorOpen = true;
	}

	async function remove(name: string) {
		try {
			await deleteParameter(name);
			toast.success('Parameter deleted.');
			confirmDelete = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}

	function typeColor(t: ParameterType): string {
		if (t === 'SecureString') return 'text-yellow-500';
		if (t === 'StringList') return 'text-blue-500';
		return 'text-muted-foreground';
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex flex-wrap items-end justify-between gap-3">
		<div class="flex flex-wrap items-end gap-3">
			<div class="flex flex-col gap-1">
				<Label for="ssm-path" class="text-xs uppercase tracking-wide text-muted-foreground">
					Path / search
				</Label>
				<Input
					id="ssm-path"
					bind:value={pathFilter}
					placeholder="/app/prod or partial name"
					class="h-8 w-64 font-mono text-xs"
				/>
			</div>
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
		</div>
		<Button size="sm" onclick={openCreate}>
			<PlusIcon /> Put parameter
		</Button>
	</div>

	{#snippet typeCell(p: Parameter)}
		<span class={`font-mono text-[11px] ${typeColor(p.type)}`}>{p.type}</span>
	{/snippet}

	{#snippet versionCell(p: Parameter)}
		<Badge variant="outline" class="h-4 px-1.5 text-[10px]">v{p.version}</Badge>
	{/snippet}

	{#snippet actionsCell(p: Parameter)}
		<div class="flex items-center justify-end gap-1">
			<Button
				size="xs"
				variant="ghost"
				aria-label="Edit parameter"
				onclick={(e: MouseEvent) => {
					e.stopPropagation();
					openEdit(p.name);
				}}
			>
				<SettingsIcon />
			</Button>
			<Button
				size="xs"
				variant="ghost"
				class="text-destructive hover:text-destructive"
				aria-label="Delete parameter"
				onclick={(e: MouseEvent) => {
					e.stopPropagation();
					confirmDelete = p.name;
				}}
			>
				<Trash2Icon />
			</Button>
		</div>
	{/snippet}

	<DataTable
		rows={parameters}
		{loading}
		rowKey={(p) => p.name}
		onRowClick={(p) => openEdit(p.name)}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'type', label: 'Type', width: '130px', cell: typeCell },
			{ key: 'version', label: 'Version', width: '80px', cell: versionCell },
			{ key: 'lastModifiedDate', label: 'Last modified', width: '220px' },
			{ key: 'actions', label: '', width: '90px', align: 'right', cell: actionsCell }
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={SettingsIcon}
				title="No parameters"
				description="Store configuration and secrets hierarchically (e.g. /app/prod/db-url)."
			>
				{#snippet action()}
					<Button onclick={openCreate}>
						<PlusIcon /> Add your first parameter
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

<ParameterEditor
	open={editorOpen}
	onOpenChange={(o) => (editorOpen = o)}
	paramName={editingName}
	onSaved={load}
/>

<Dialog
	open={confirmDelete !== null}
	onOpenChange={(o) => {
		if (!o) confirmDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete parameter?</DialogTitle>
			<DialogDescription>
				Permanently removes <span class="font-mono">{confirmDelete}</span>.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDelete = null)}>Cancel</Button>
			<Button variant="destructive" onclick={() => confirmDelete && remove(confirmDelete)}>
				<Trash2Icon /> Delete
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
