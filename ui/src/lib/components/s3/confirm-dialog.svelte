<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		open: boolean;
		title: string;
		description: string;
		confirmLabel?: string;
		busy?: boolean;
		onConfirm: () => void;
		onClose: () => void;
	}

	let {
		open = $bindable(false),
		title,
		description,
		confirmLabel = 'Delete',
		busy = false,
		onConfirm,
		onClose
	}: Props = $props();
</script>

<Dialog bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>{title}</DialogTitle>
			<DialogDescription>{description}</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={onClose} disabled={busy}>Cancel</Button>
			<Button variant="destructive" onclick={onConfirm} disabled={busy}>
				{#if busy}
					<Loader2 class="size-3.5 animate-spin" />
				{/if}
				{confirmLabel}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
