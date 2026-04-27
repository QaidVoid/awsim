<script lang="ts">
	import { Skeleton } from '$lib/components/ui/skeleton';

	interface Props {
		rows?: number;
		class?: string;
	}

	let { rows = 4, class: className = '' }: Props = $props();
</script>

<div class={'flex flex-col gap-2 ' + className}>
	{#each Array(rows) as _, i (i)}
		<div class="flex items-center gap-3 rounded-md border border-border bg-card/40 px-3 py-2.5">
			<Skeleton class="size-5 shrink-0 rounded" />
			<div class="flex flex-1 flex-col gap-1">
				<Skeleton class="h-3 w-1/3" />
				<Skeleton class="h-2 w-1/2" />
			</div>
			<Skeleton class="size-5 shrink-0 rounded" />
		</div>
	{/each}
</div>
