<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import KeyIcon from '@lucide/svelte/icons/key';
	import { toast } from 'svelte-sonner';
	import { describeActivations, type Activation } from '$lib/api/ssm';

	let activations = $state<Activation[]>([]);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			activations = await describeActivations();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load activations');
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
		<h3 class="text-sm font-semibold">Hybrid activations ({activations.length})</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	{#snippet expiredCell(a: Activation)}
		{#if a.expired}
			<Badge variant="outline" class="h-4 px-1.5 text-[10px] text-destructive">expired</Badge>
		{:else}
			<Badge variant="outline" class="h-4 px-1.5 text-[10px] text-green-500">active</Badge>
		{/if}
	{/snippet}

	{#snippet regCell(a: Activation)}
		<span class="font-mono text-xs text-muted-foreground">
			{a.registrationsCount ?? 0}/{a.registrationLimit ?? '∞'}
		</span>
	{/snippet}

	<DataTable
		rows={activations}
		{loading}
		rowKey={(a) => a.activationId}
		columns={[
			{ key: 'activationId', label: 'Activation ID', mono: true },
			{ key: 'description', label: 'Description' },
			{ key: 'iamRole', label: 'IAM role', mono: true, width: '180px' },
			{ key: 'registrations', label: 'Registrations', width: '130px', cell: regCell },
			{ key: 'expired', label: 'Status', width: '100px', cell: expiredCell },
			{ key: 'expirationDate', label: 'Expires', width: '210px' }
		]}
	>
		{#snippet empty()}
			<EmptyState
				icon={KeyIcon}
				title="No activations"
				description="Activations let on-prem servers register as managed instances."
			/>
		{/snippet}
	</DataTable>
</div>
