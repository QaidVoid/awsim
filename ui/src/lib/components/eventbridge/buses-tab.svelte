<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import RouteIcon from '@lucide/svelte/icons/route';
	import SendIcon from '@lucide/svelte/icons/send';
	import { toast } from 'svelte-sonner';
	import { listEventBuses, type EventBus } from '$lib/api/eventbridge';

	interface Props {
		selectedBus: string;
		onSelect: (busName: string) => void;
		onSendEvent: (busName: string) => void;
	}

	let { selectedBus, onSelect, onSendEvent }: Props = $props();

	let buses = $state<EventBus[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			buses = await listEventBuses();
			if (!selectedBus && buses.length > 0) {
				onSelect(buses[0].name);
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load event buses');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">Event buses ({buses.length})</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	{#if buses.length === 0 && !loading}
		<EmptyState
			icon={RouteIcon}
			title="No event buses"
			description="EventBridge always provides a `default` bus. Create custom buses via the AWS CLI."
		/>
	{:else}
		<ul class="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
			{#each buses as bus (bus.arn)}
				{@const isSelected = selectedBus === bus.name}
				<li>
					<button
						type="button"
						class="flex w-full flex-col items-start gap-1 rounded-md border border-border bg-card/40 px-3 py-3 text-left transition-colors hover:bg-muted/40 aria-pressed:border-primary"
						aria-pressed={isSelected}
						onclick={() => onSelect(bus.name)}
					>
						<div class="flex items-center justify-between gap-2">
							<span class="truncate font-mono text-xs font-medium">{bus.name}</span>
							{#if bus.name === 'default'}
								<Badge variant="outline" class="h-4 px-1.5 text-[10px]">Default</Badge>
							{/if}
						</div>
						<p class="truncate font-mono text-[10px] text-muted-foreground">
							{bus.arn}
						</p>
					</button>
					<div class="mt-1 flex justify-end">
						<Button
							size="xs"
							variant="ghost"
							onclick={() => onSendEvent(bus.name)}
						>
							<SendIcon />
							Send event
						</Button>
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</div>
