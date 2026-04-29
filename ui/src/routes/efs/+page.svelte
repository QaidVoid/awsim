<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import FileSystemsList from '$lib/components/efs/file-systems-list.svelte';
	import CreateFileSystemDialog from '$lib/components/efs/create-file-system-dialog.svelte';
	import FileSystemDetailSheet from '$lib/components/efs/file-system-detail-sheet.svelte';
	import type { FileSystem } from '$lib/api/efs';

	let createOpen = $state(false);
	let detailOpen = $state(false);
	let detailFs = $state<FileSystem | null>(null);
	let refreshKey = $state(0);

	function refresh() {
		refreshKey += 1;
	}

	function openDetail(fs: FileSystem) {
		detailFs = fs;
		detailOpen = true;
	}
</script>

<ServicePage
	title="EFS"
	description="Elastic File System — file systems, mount targets, and access points."
>
	{#snippet actions()}
		<Button size="sm" onclick={() => (createOpen = true)}>
			<PlusIcon />
			New file system
		</Button>
	{/snippet}

	<FileSystemsList
		onSelect={openDetail}
		onCreate={() => (createOpen = true)}
		{refreshKey}
	/>
</ServicePage>

<CreateFileSystemDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={refresh}
/>

<FileSystemDetailSheet
	open={detailOpen}
	fs={detailFs}
	onOpenChange={(o) => (detailOpen = o)}
	onChanged={refresh}
/>
