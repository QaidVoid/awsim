<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import UsersIcon from '@lucide/svelte/icons/users';
	import { toast } from 'svelte-sonner';
	import {
		listPermissionSets,
		describePermissionSet,
		listAccountAssignments,
		type Instance,
		type AccountAssignment,
		type PermissionSet
	} from '$lib/api/sso-admin';

	interface Props {
		instance: Instance | null;
	}

	let { instance }: Props = $props();

	let accountId = $state('000000000000');
	let assignments = $state<AccountAssignment[]>([]);
	let permissionSets = $state<PermissionSet[]>([]);
	let loading = $state(false);

	function psName(arn: string): string {
		const ps = permissionSets.find((p) => p.permissionSetArn === arn);
		return ps?.name ?? '';
	}

	async function load() {
		if (!instance) {
			assignments = [];
			permissionSets = [];
			return;
		}
		if (!accountId.trim()) return;
		loading = true;
		try {
			const arns = await listPermissionSets(instance.instanceArn);
			const details = await Promise.all(
				arns.map((arn) =>
					describePermissionSet(instance.instanceArn, arn).catch(
						() => ({ permissionSetArn: arn, name: '' }) as PermissionSet
					)
				)
			);
			permissionSets = details;
			const all: AccountAssignment[] = [];
			for (const arn of arns) {
				try {
					const a = await listAccountAssignments(
						instance.instanceArn,
						accountId.trim(),
						arn
					);
					all.push(...a);
				} catch {
					// skip
				}
			}
			assignments = all;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load assignments');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		instance;
		load();
	});
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex flex-wrap items-end gap-3">
		<div class="flex flex-col gap-1">
			<Label for="sso-account" class="text-xs uppercase tracking-wide text-muted-foreground">
				Account ID
			</Label>
			<Input
				id="sso-account"
				bind:value={accountId}
				placeholder="000000000000"
				class="h-8 w-44 font-mono text-xs"
			/>
		</div>
		<Button size="sm" onclick={load} disabled={loading || !instance || !accountId.trim()}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Reload
		</Button>
	</div>

	{#snippet psCell(a: AccountAssignment)}
		<Badge variant="outline" class="font-mono text-[10px]">
			{psName(a.permissionSetArn) || a.permissionSetArn.slice(-12)}
		</Badge>
	{/snippet}

	{#if !instance}
		<EmptyState
			icon={UsersIcon}
			title="No instance selected"
			description="Select an Identity Center instance to view account assignments."
		/>
	{:else}
		<DataTable
			rows={assignments}
			{loading}
			rowKey={(a, i) => `${a.permissionSetArn}-${a.principalId}-${i}`}
			columns={[
				{ key: 'accountId', label: 'Account', mono: true, width: '160px' },
				{ key: 'permissionSet', label: 'Permission set', cell: psCell },
				{ key: 'principalId', label: 'Principal', mono: true },
				{ key: 'principalType', label: 'Type', width: '110px' }
			]}
		>
			{#snippet empty()}
				<EmptyState
					icon={UsersIcon}
					title="No assignments"
					description="No principals are assigned to permission sets in this account."
				/>
			{/snippet}
		</DataTable>
	{/if}
</div>
