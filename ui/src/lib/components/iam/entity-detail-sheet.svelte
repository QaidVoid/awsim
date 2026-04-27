<script lang="ts">
	import type { Snippet } from 'svelte';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		title: string;
		subtitle?: string;
		children: Snippet;
	}

	let { open = $bindable(), onOpenChange, title, subtitle, children }: Props = $props();
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-2xl overflow-y-auto sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle class="truncate">{title}</SheetTitle>
			{#if subtitle}
				<SheetDescription class="truncate font-mono text-xs">{subtitle}</SheetDescription>
			{/if}
		</SheetHeader>
		<div class="px-6 pb-6">
			{@render children()}
		</div>
	</SheetContent>
</Sheet>
