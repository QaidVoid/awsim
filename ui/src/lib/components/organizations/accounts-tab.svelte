<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listAccounts,
		accountStatusVariant,
		type Account
	} from '$lib/api/organizations';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Users from '@lucide/svelte/icons/users';
	import Plus from '@lucide/svelte/icons/plus';
	import { toast } from 'svelte-sonner';
	import CreateAccountDialog from './create-account-dialog.svelte';

	interface Props {
		onSelect: (account: Account) => void;
	}

	let { onSelect }: Props = $props();

	let accounts = $state<Account[]>([]);
	let loading = $state(true);
	let createOpen = $state(false);

	async function reload() {
		loading = true;
		try {
			const r = await listAccounts();
			accounts = r.accounts;
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'Failed to load accounts');
		} finally {
			loading = false;
		}
	}

	onMount(reload);
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex items-center justify-between border-b border-border px-4 py-2">
		<div class="text-xs text-muted-foreground">
			{accounts.length} account{accounts.length === 1 ? '' : 's'}
		</div>
		<div class="flex items-center gap-2">
			<Button type="button" variant="outline" size="sm" onclick={reload} disabled={loading}>
				<RefreshCw class={loading ? 'animate-spin' : ''} />
				Refresh
			</Button>
			<Button type="button" size="sm" onclick={() => (createOpen = true)}>
				<Plus />
				Create account
			</Button>
		</div>
	</header>

	<div class="min-h-0 flex-1 overflow-auto">
		{#if loading && accounts.length === 0}
			<div class="space-y-2 p-4">
				{#each Array(4) as _, i (i)}
					<Skeleton class="h-7 w-full" />
				{/each}
			</div>
		{:else if accounts.length === 0}
			<div class="p-6">
				<EmptyState
					icon={Users}
					title="No accounts"
					description="Member accounts belong to the organization and can be grouped into OUs."
				>
					{#snippet action()}
						<Button onclick={() => (createOpen = true)}>
							<Plus />
							Create your first account
						</Button>
					{/snippet}
				</EmptyState>
			</div>
		{:else}
			<table class="w-full text-sm">
				<thead class="sticky top-0 z-10 border-b border-border bg-background/95 backdrop-blur-sm">
					<tr>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">ID</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Name</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Email</th>
						<th class="px-4 py-2 text-left font-medium text-muted-foreground">Status</th>
					</tr>
				</thead>
				<tbody>
					{#each accounts as a (a.id || a.arn)}
						<tr
							class="cursor-pointer border-b border-border/40 hover:bg-muted/30"
							onclick={() => onSelect(a)}
						>
							<td class="px-4 py-2 font-mono text-xs">{a.id}</td>
							<td class="px-4 py-2 font-mono text-xs">{a.name}</td>
							<td class="px-4 py-2 font-mono text-[11px] text-muted-foreground">{a.email}</td>
							<td class="px-4 py-2">
								<Badge variant={accountStatusVariant(a.status)}>{a.status}</Badge>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>

<CreateAccountDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={reload}
/>
