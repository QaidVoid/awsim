<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState, ListSkeleton } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import ContactIcon from '@lucide/svelte/icons/contact';
	import { toast } from 'svelte-sonner';
	import {
		listContactLists,
		listContacts,
		type ContactList,
		type Contact
	} from '$lib/api/ses';

	let lists = $state<ContactList[]>([]);
	let loading = $state(false);
	let selected = $state<string | null>(null);
	let contacts = $state<Contact[]>([]);
	let contactsLoading = $state(false);

	async function load() {
		loading = true;
		try {
			lists = await listContactLists();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load contact lists');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});

	async function open(name: string) {
		selected = name;
		contacts = [];
		contactsLoading = true;
		try {
			contacts = await listContacts(name);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load contacts');
		} finally {
			contactsLoading = false;
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">Contact lists ({lists.length})</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="grid gap-4 lg:grid-cols-2">
		<DataTable
			rows={lists}
			{loading}
			rowKey={(l) => l.name}
			onRowClick={(l) => open(l.name)}
			columns={[
				{ key: 'name', label: 'List', mono: true },
				{ key: 'lastUpdatedTimestamp', label: 'Last updated', width: '230px' }
			]}
		>
			{#snippet empty()}
				<EmptyState
					icon={ContactIcon}
					title="No contact lists"
					description="Group recipients into reusable lists for marketing sends."
				/>
			{/snippet}
		</DataTable>

		<aside class="rounded-md border border-border bg-card/40 p-3" aria-label="Contacts">
			<p class="mb-2 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
				Contacts {selected ? `· ${selected}` : ''}
			</p>
			{#if !selected}
				<p class="text-xs text-muted-foreground">Select a list to view its contacts.</p>
			{:else if contactsLoading}
				<ListSkeleton rows={3} />
			{:else if contacts.length === 0}
				<p class="text-xs text-muted-foreground">No contacts in this list.</p>
			{:else}
				<ul class="flex flex-col gap-1.5">
					{#each contacts as c (c.emailAddress)}
						<li class="flex items-center justify-between gap-2 text-xs">
							<span class="truncate font-mono">{c.emailAddress}</span>
							{#if c.unsubscribeAll}
								<Badge variant="outline" class="h-4 px-1.5 text-[10px] text-destructive">
									unsubscribed
								</Badge>
							{/if}
						</li>
					{/each}
				</ul>
			{/if}
		</aside>
	</div>
</div>
