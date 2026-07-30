<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { EmptyState, ListSkeleton } from '$lib/components/service';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import RouteIcon from '@lucide/svelte/icons/route';
	import SendIcon from '@lucide/svelte/icons/send';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { toast } from 'svelte-sonner';
	import {
		listEventBuses,
		createEventBus,
		deleteEventBus,
		type EventBus,
	} from '$lib/api/eventbridge';

	interface Props {
		selectedBus: string;
		onSelect: (busName: string) => void;
		onSendEvent: (busName: string) => void;
	}

	let { selectedBus, onSelect, onSendEvent }: Props = $props();

	let buses = $state<EventBus[]>([]);
	let loading = $state(false);
	let createOpen = $state(false);
	let newName = $state('');
	let creating = $state(false);
	let pendingDelete = $state<string | null>(null);

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

	async function create() {
		const name = newName.trim();
		if (!name) {
			toast.error('Bus name is required.');
			return;
		}
		creating = true;
		try {
			await createEventBus(name);
			toast.success('Event bus created.');
			newName = '';
			createOpen = false;
			await load();
			onSelect(name);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to create event bus');
		} finally {
			creating = false;
		}
	}

	async function remove(name: string) {
		try {
			await deleteEventBus(name);
			toast.success('Event bus deleted.');
			pendingDelete = null;
			// Deleting the selected bus would leave the rules tab querying
			// a bus that no longer exists, so fall back to default.
			if (selectedBus === name) onSelect('default');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete event bus');
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">Event buses ({buses.length})</h3>
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
				<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button size="sm" onclick={() => (createOpen = true)}>
				<PlusIcon />
				New bus
			</Button>
		</div>
	</div>

	{#if loading && buses.length === 0}
		<ListSkeleton rows={3} />
	{:else if buses.length === 0}
		<EmptyState
			icon={RouteIcon}
			title="No event buses"
			description="Event buses route events from sources to rule targets. A built-in default bus is always available even when no custom buses exist."
		>
			{#snippet action()}
				<Button onclick={() => (createOpen = true)}>
					<PlusIcon />
					Create event bus
				</Button>
			{/snippet}
		</EmptyState>
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
						<!-- `items-start` makes these shrink to content, so without an
						     explicit width `truncate` has nothing to truncate against and
						     a long ARN widens the whole card instead. -->
						<div class="flex w-full min-w-0 items-center justify-between gap-2">
							<span class="truncate font-mono text-xs font-medium">{bus.name}</span>
							{#if bus.name === 'default'}
								<Badge variant="outline" class="h-4 px-1.5 text-[10px]">Default</Badge>
							{/if}
						</div>
						<p class="w-full min-w-0 truncate font-mono text-[10px] text-muted-foreground">
							{bus.arn}
						</p>
					</button>
					<div class="mt-1 flex justify-end gap-1">
						<Button
							size="xs"
							variant="ghost"
							onclick={() => onSendEvent(bus.name)}
						>
							<SendIcon />
							Send event
						</Button>
						{#if bus.name !== 'default'}
							<Button
								size="xs"
								variant="ghost"
								class="text-destructive hover:text-destructive"
								onclick={() => (pendingDelete = bus.name)}
								aria-label="Delete event bus"
							>
								<Trash2Icon />
							</Button>
						{/if}
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<Dialog open={createOpen} onOpenChange={(o) => (createOpen = o)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New event bus</DialogTitle>
			<DialogDescription>
				A separate routing domain. Rules and events on one bus never reach another.
			</DialogDescription>
		</DialogHeader>
		<div class="flex flex-col gap-1 px-4">
			<Label for="evb-bus-name">Name</Label>
			<Input id="evb-bus-name" bind:value={newName} placeholder="app-events" />
		</div>
		<DialogFooter>
			<Button variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
			<Button onclick={create} disabled={creating || !newName.trim()}>
				{creating ? 'Creating...' : 'Create bus'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<Dialog
	open={pendingDelete !== null}
	onOpenChange={(o) => {
		if (!o) pendingDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete event bus?</DialogTitle>
			<DialogDescription>
				Removes <span class="font-mono">{pendingDelete}</span> along with every rule on it.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (pendingDelete = null)}>Cancel</Button>
			<Button variant="destructive" onclick={() => pendingDelete && remove(pendingDelete)}>
				Delete
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
