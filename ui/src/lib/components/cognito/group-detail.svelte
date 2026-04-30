<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		listUsersInGroup,
		listPoolUsers,
		adminAddUserToGroup,
		adminRemoveUserFromGroup,
		type CognitoUserSummary
	} from '$lib/api/cognito';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import {
		Popover,
		PopoverContent,
		PopoverTrigger
	} from '$lib/components/ui/popover';
	import { Input } from '$lib/components/ui/input';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		poolId: string;
		groupName: string;
	}

	let { poolId, groupName }: Props = $props();

	let members = $state<CognitoUserSummary[]>([]);
	let allUsers = $state<CognitoUserSummary[]>([]);
	let loading = $state(true);
	let addOpen = $state(false);
	let addFilter = $state('');

	const candidates = $derived(
		allUsers
			.filter((u) => !members.some((m) => m.username === u.username))
			.filter((u) =>
				addFilter.trim()
					? u.username.toLowerCase().includes(addFilter.trim().toLowerCase())
					: true
			)
	);

	onMount(load);

	async function load() {
		loading = true;
		try {
			const [m, all] = await Promise.all([
				listUsersInGroup(poolId, groupName),
				listPoolUsers(poolId)
			]);
			members = m;
			allUsers = all;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load members');
		} finally {
			loading = false;
		}
	}

	async function addMember(u: CognitoUserSummary) {
		try {
			await adminAddUserToGroup(poolId, u.username, groupName);
			toast.success(`Added ${u.username}`);
			addOpen = false;
			addFilter = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Add failed');
		}
	}

	async function removeMember(u: CognitoUserSummary) {
		try {
			await adminRemoveUserFromGroup(poolId, u.username, groupName);
			toast.success(`Removed ${u.username}`);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Remove failed');
		}
	}
</script>

<div class="space-y-3 rounded border border-border/60 bg-muted/20 px-3 py-3">
	<div class="flex items-center justify-between">
		<span class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
			Members ({members.length})
		</span>
		<Popover bind:open={addOpen}>
			<PopoverTrigger>
				<Button size="xs" variant="outline">
					<Plus class="size-3" /> Add member
				</Button>
			</PopoverTrigger>
			<PopoverContent class="w-72 p-2">
				<Input
					placeholder="Filter users..."
					bind:value={addFilter}
					class="mb-2 h-7"
				/>
				{#if candidates.length === 0}
					<p class="px-2 py-1.5 text-xs text-muted-foreground">No matching users.</p>
				{:else}
					<ul class="max-h-64 space-y-0.5 overflow-y-auto">
						{#each candidates as u (u.username)}
							<li>
								<button
									type="button"
									class="w-full rounded px-2 py-1.5 text-left text-sm hover:bg-muted"
									onclick={() => addMember(u)}
								>
									{u.username}
								</button>
							</li>
						{/each}
					</ul>
				{/if}
			</PopoverContent>
		</Popover>
	</div>
	{#if loading}
		<p class="text-xs text-muted-foreground">
			<Loader2 class="inline size-3 animate-spin" /> Loading...
		</p>
	{:else if members.length === 0}
		<p class="text-xs text-muted-foreground">No members.</p>
	{:else}
		<div class="flex flex-wrap gap-1.5">
			{#each members as u (u.username)}
				<Badge variant="secondary" class="gap-1.5 pr-1 pl-2 py-1">
					<span>{u.username}</span>
					<button
						type="button"
						class="rounded-sm hover:bg-foreground/10"
						onclick={() => removeMember(u)}
						title="Remove member"
						aria-label="Remove {u.username}"
					>
						<X class="size-3" />
					</button>
				</Badge>
			{/each}
		</div>
	{/if}
</div>
