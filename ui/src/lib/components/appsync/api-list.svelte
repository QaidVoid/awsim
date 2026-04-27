<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import SearchIcon from '@lucide/svelte/icons/search';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { cn } from '$lib/utils';
	import type { GraphqlApi } from '$lib/api/appsync';

	interface Props {
		apis: GraphqlApi[];
		selectedId: string | null;
		onSelect: (id: string) => void;
		onCreate: () => void;
	}

	let { apis, selectedId, onSelect, onCreate }: Props = $props();

	let filter = $state('');
	let filtered = $derived(
		filter.trim() === ''
			? apis
			: apis.filter((a) => a.name.toLowerCase().includes(filter.trim().toLowerCase()))
	);

	function authVariant(t: string): 'secondary' | 'outline' {
		if (t === 'API_KEY' || t === 'AWS_IAM' || t === 'IAM') return 'secondary';
		return 'outline';
	}
</script>

<div class="flex h-full min-h-0 flex-col border-r border-border">
	<div class="flex items-center gap-2 border-b border-border px-3 py-2">
		<div class="relative flex-1">
			<SearchIcon
				class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground"
			/>
			<Input
				type="search"
				placeholder="Filter APIs"
				bind:value={filter}
				class="h-8 pl-7 text-xs"
			/>
		</div>
		<Button size="icon-sm" variant="outline" onclick={onCreate} aria-label="Create API">
			<PlusIcon />
		</Button>
	</div>
	<div class="min-h-0 flex-1 overflow-y-auto">
		{#each filtered as api (api.apiId)}
			{@const isSelected = selectedId === api.apiId}
			<button
				type="button"
				class={cn(
					'block w-full border-b border-border/40 px-3 py-2 text-left transition-colors',
					isSelected ? 'bg-muted' : 'hover:bg-muted/50'
				)}
				onclick={() => onSelect(api.apiId)}
			>
				<div class="flex items-center gap-2">
					<span class="truncate font-mono text-xs font-medium">{api.name}</span>
				</div>
				<div class="mt-0.5 flex items-center gap-1.5">
					<Badge variant={authVariant(api.authenticationType)} class="h-4 px-1.5 text-[10px]">
						{api.authenticationType || 'NONE'}
					</Badge>
					<span class="truncate font-mono text-[10px] text-muted-foreground">
						{api.apiId}
					</span>
				</div>
			</button>
		{:else}
			<div class="px-3 py-8 text-center text-xs text-muted-foreground">
				{filter ? 'No matches.' : 'No APIs.'}
			</div>
		{/each}
	</div>
</div>
