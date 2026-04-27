<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import UploadCloud from '@lucide/svelte/icons/upload-cloud';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import { cn } from '$lib/utils';

	interface Props {
		disabled?: boolean;
		uploading?: boolean;
		onFiles: (files: File[]) => void;
	}

	let { disabled = false, uploading = false, onFiles }: Props = $props();

	let fileInput: HTMLInputElement | null = $state(null);
	let dragActive = $state(false);

	function handleDrop(e: DragEvent) {
		e.preventDefault();
		dragActive = false;
		if (disabled || !e.dataTransfer) return;
		const files = Array.from(e.dataTransfer.files);
		if (files.length > 0) onFiles(files);
	}

	function handleDragOver(e: DragEvent) {
		e.preventDefault();
		if (!disabled) dragActive = true;
	}

	function handleDragLeave() {
		dragActive = false;
	}

	function handleSelect(e: Event) {
		const target = e.target as HTMLInputElement;
		if (!target.files) return;
		const files = Array.from(target.files);
		if (files.length > 0) onFiles(files);
		target.value = '';
	}
</script>

<div
	class={cn(
		'flex shrink-0 items-center justify-between gap-3 border-t border-border bg-background/40 px-4 py-2.5 transition-colors',
		dragActive && 'bg-primary/10',
		disabled && 'opacity-60'
	)}
	role="region"
	aria-label="Upload files"
	ondrop={handleDrop}
	ondragover={handleDragOver}
	ondragleave={handleDragLeave}
>
	<div class="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
		{#if uploading}
			<Loader2 class="size-3.5 shrink-0 animate-spin" />
			<span>Uploading...</span>
		{:else}
			<UploadCloud class="size-3.5 shrink-0" />
			<span class="truncate">
				{dragActive ? 'Release to upload' : 'Drag files here or use the button'}
			</span>
		{/if}
	</div>

	<input
		bind:this={fileInput}
		type="file"
		multiple
		hidden
		onchange={handleSelect}
		{disabled}
	/>
	<Button
		variant="outline"
		size="sm"
		{disabled}
		onclick={() => fileInput?.click()}
	>
		<UploadCloud class="size-3.5" />
		Upload
	</Button>
</div>
