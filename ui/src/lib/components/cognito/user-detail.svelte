<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		adminGetUser,
		adminListGroupsForUser,
		adminAddUserToGroup,
		adminRemoveUserFromGroup,
		adminUpdateUserAttributes,
		listGroups,
		type CognitoUser,
		type CognitoGroup
	} from '$lib/api/cognito';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import {
		Popover,
		PopoverContent,
		PopoverTrigger
	} from '$lib/components/ui/popover';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		poolId: string;
		username: string;
	}

	let { poolId, username }: Props = $props();

	let user = $state<CognitoUser | null>(null);
	let groups = $state<CognitoGroup[]>([]);
	let allGroups = $state<CognitoGroup[]>([]);
	let loading = $state(true);
	let editing = $state<{ name: string; value: string } | null>(null);
	let editValue = $state('');
	let savingAttr = $state(false);
	let newAttrName = $state('');
	let newAttrValue = $state('');
	let addGroupOpen = $state(false);

	const availableGroups = $derived(
		allGroups.filter((g) => !groups.some((existing) => existing.name === g.name))
	);

	onMount(load);

	async function load() {
		loading = true;
		try {
			const [u, g, all] = await Promise.all([
				adminGetUser(poolId, username),
				adminListGroupsForUser(poolId, username),
				listGroups(poolId)
			]);
			user = u;
			groups = g;
			allGroups = all;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load user');
		} finally {
			loading = false;
		}
	}

	function startEdit(name: string, value: string) {
		editing = { name, value };
		editValue = value;
	}

	async function saveEdit() {
		if (!editing) return;
		savingAttr = true;
		try {
			await adminUpdateUserAttributes({
				poolId,
				username,
				attributes: [{ name: editing.name, value: editValue }]
			});
			toast.success(`Updated ${editing.name}`);
			editing = null;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			savingAttr = false;
		}
	}

	async function addAttribute() {
		if (!newAttrName.trim() || !newAttrValue.trim()) return;
		savingAttr = true;
		try {
			await adminUpdateUserAttributes({
				poolId,
				username,
				attributes: [{ name: newAttrName.trim(), value: newAttrValue.trim() }]
			});
			toast.success(`Added ${newAttrName.trim()}`);
			newAttrName = '';
			newAttrValue = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Add failed');
		} finally {
			savingAttr = false;
		}
	}

	async function joinGroup(g: CognitoGroup) {
		try {
			await adminAddUserToGroup(poolId, username, g.name);
			toast.success(`Added to ${g.name}`);
			addGroupOpen = false;
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Add failed');
		}
	}

	async function leaveGroup(g: CognitoGroup) {
		try {
			await adminRemoveUserFromGroup(poolId, username, g.name);
			toast.success(`Removed from ${g.name}`);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Remove failed');
		}
	}
</script>

<div class="space-y-4 rounded border border-border/60 bg-muted/20 px-3 py-3">
	{#if loading}
		<p class="text-xs text-muted-foreground">
			<Loader2 class="inline size-3 animate-spin" /> Loading...
		</p>
	{:else if user}
		<div>
			<div class="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
				Attributes
			</div>
			<ul class="space-y-1.5">
				{#each user.attributes as a (a.name)}
					<li class="flex items-center gap-2 text-sm">
						<span class="w-32 shrink-0 font-mono text-xs text-muted-foreground">{a.name}</span>
						{#if editing?.name === a.name}
							<Input bind:value={editValue} class="h-7 flex-1" />
							<Button size="xs" onclick={saveEdit} disabled={savingAttr}>Save</Button>
							<Button
								size="xs"
								variant="ghost"
								onclick={() => (editing = null)}
								disabled={savingAttr}>Cancel</Button
							>
						{:else}
							<span class="flex-1 truncate font-mono text-xs">{a.value}</span>
							<Button
								variant="ghost"
								size="xs"
								onclick={() => startEdit(a.name, a.value)}
								disabled={a.name === 'sub'}
							>
								Edit
							</Button>
						{/if}
					</li>
				{/each}
				<li class="flex items-center gap-2 pt-1">
					<Input
						placeholder="custom:foo or given_name"
						bind:value={newAttrName}
						class="h-7 w-48 font-mono text-xs"
					/>
					<Input bind:value={newAttrValue} placeholder="value" class="h-7 flex-1 text-xs" />
					<Button
						size="xs"
						onclick={addAttribute}
						disabled={savingAttr || !newAttrName.trim() || !newAttrValue.trim()}
					>
						<Plus class="size-3" />
					</Button>
				</li>
			</ul>
		</div>

		<div>
			<div class="mb-2 flex items-center justify-between">
				<span class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
					Groups
				</span>
				<Popover bind:open={addGroupOpen}>
					<PopoverTrigger>
						<Button size="xs" variant="outline" disabled={availableGroups.length === 0}>
							<Plus class="size-3" /> Add to group
						</Button>
					</PopoverTrigger>
					<PopoverContent class="w-56 p-1">
						{#if availableGroups.length === 0}
							<p class="px-2 py-1.5 text-xs text-muted-foreground">No more groups.</p>
						{:else}
							<ul class="space-y-0.5">
								{#each availableGroups as g (g.name)}
									<li>
										<button
											type="button"
											class="w-full rounded px-2 py-1.5 text-left text-sm hover:bg-muted"
											onclick={() => joinGroup(g)}
										>
											{g.name}
										</button>
									</li>
								{/each}
							</ul>
						{/if}
					</PopoverContent>
				</Popover>
			</div>
			{#if groups.length === 0}
				<p class="text-xs text-muted-foreground">No group memberships.</p>
			{:else}
				<div class="flex flex-wrap gap-1.5">
					{#each groups as g (g.name)}
						<Badge variant="secondary" class="gap-1.5 pr-1 pl-2 py-1">
							<span>{g.name}</span>
							<button
								type="button"
								class="rounded-sm hover:bg-foreground/10"
								onclick={() => leaveGroup(g)}
								title="Remove from group"
								aria-label="Remove from group {g.name}"
							>
								<X class="size-3" />
							</button>
						</Badge>
					{/each}
				</div>
			{/if}
		</div>
	{/if}
</div>
