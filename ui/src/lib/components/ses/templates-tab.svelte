<script lang="ts">
	import { Button } from '$lib/components/ui/button';
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
	import FileEditIcon from '@lucide/svelte/icons/file-edit';
	import { toast } from 'svelte-sonner';
	import { listTemplates, deleteTemplate, type Template } from '$lib/api/ses';
	import TemplateEditor from './template-editor.svelte';

	let templates = $state<Template[]>([]);
	let loading = $state(false);
	let editorOpen = $state(false);
	let editingName = $state<string | null>(null);
	let confirmDelete = $state<string | null>(null);

	async function load() {
		loading = true;
		try {
			templates = await listTemplates();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load templates');
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
			await deleteTemplate(name);
			toast.success('Template deleted.');
			confirmDelete = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">Email templates ({templates.length})</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={openCreate}>
				<PlusIcon /> New template
			</Button>
		</div>
	</div>

	{#snippet actionsCell(t: Template)}
		<div class="flex justify-end gap-1">
			<Button
				size="xs"
				variant="ghost"
				aria-label="Edit template"
				onclick={(e: MouseEvent) => {
					e.stopPropagation();
					openEdit(t.name);
				}}
			>
				<FileEditIcon />
			</Button>
			<Button
				size="xs"
				variant="ghost"
				class="text-destructive hover:text-destructive"
				aria-label="Delete template"
				onclick={(e: MouseEvent) => {
					e.stopPropagation();
					confirmDelete = t.name;
				}}
			>
				<Trash2Icon />
			</Button>
		</div>
	{/snippet}

	<DataTable
		rows={templates}
		{loading}
		rowKey={(t) => t.name}
		onRowClick={(t) => openEdit(t.name)}
		columns={[
			{ key: 'name', label: 'Name', mono: true },
			{ key: 'createdTimestamp', label: 'Created', width: '230px' },
			{ key: 'actions', label: '', width: '90px', align: 'right', cell: actionsCell }
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={FileEditIcon}
				title="No templates"
				description="Reusable email templates with variable substitution."
			>
				{#snippet action()}
					<Button onclick={openCreate}>
						<PlusIcon /> Create your first template
					</Button>
				{/snippet}
			</EmptyState>
		{/snippet}
	</DataTable>
</div>

<TemplateEditor
	open={editorOpen}
	onOpenChange={(o) => (editorOpen = o)}
	templateName={editingName}
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
			<DialogTitle>Delete template?</DialogTitle>
			<DialogDescription>
				Removes <span class="font-mono">{confirmDelete}</span>. Existing pending sends may fail.
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
