<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { DataTable, EmptyState } from '$lib/components/service';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlayIcon from '@lucide/svelte/icons/play';
	import RocketIcon from '@lucide/svelte/icons/rocket';
	import { toast } from 'svelte-sonner';
	import {
		listEnvironments,
		listProfiles,
		listStrategies,
		listDeployments,
		startDeployment,
		type Environment,
		type ConfigProfile,
		type DeploymentStrategy,
		type Deployment
	} from '$lib/api/appconfig';

	interface Props {
		appId: string;
		refreshKey?: number;
	}

	let { appId, refreshKey = 0 }: Props = $props();

	let envs = $state<Environment[]>([]);
	let profiles = $state<ConfigProfile[]>([]);
	let strategies = $state<DeploymentStrategy[]>([]);
	let rows = $state<Deployment[]>([]);
	let loading = $state(false);

	let selEnv = $state('');
	let selProfile = $state('');
	let selStrategy = $state('AppConfig.AllAtOnce');
	let configVersion = $state('1');
	let starting = $state(false);

	$effect(() => {
		appId;
		refreshKey;
		void load();
	});

	async function load() {
		loading = true;
		try {
			[envs, profiles, strategies] = await Promise.all([
				listEnvironments(appId),
				listProfiles(appId),
				listStrategies()
			]);
			if (!selEnv && envs[0]) selEnv = envs[0].id;
			rows = selEnv ? await listDeployments(appId, selEnv) : [];
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load deployments');
		} finally {
			loading = false;
		}
	}

	async function start() {
		if (!selEnv || !selProfile || !selStrategy) {
			return toast.error('Pick env, profile, and strategy.');
		}
		starting = true;
		try {
			await startDeployment({
				appId,
				envId: selEnv,
				profileId: selProfile,
				strategyId: selStrategy,
				configurationVersion: configVersion.trim() || '1'
			});
			toast.success('Deployment started.');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to start deployment');
		} finally {
			starting = false;
		}
	}

	function stateColor(s: string): string {
		if (s === 'COMPLETE') return 'text-green-500';
		if (s === 'ROLLED_BACK' || s === 'BAKING') return 'text-amber-500';
		if (s === 'DEPLOYING') return 'text-blue-500';
		return 'text-muted-foreground';
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="flex items-center justify-between">
		<h3 class="text-sm font-semibold">
			Deployments
			<span class="ml-1 font-normal text-muted-foreground">({rows.length})</span>
		</h3>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<div class="space-y-2 rounded-md border border-border p-3">
		<div class="text-xs font-semibold">Start deployment</div>
		<div class="grid grid-cols-2 gap-2">
			<select
				bind:value={selEnv}
				class="h-8 rounded-md border border-border bg-background px-2 text-xs"
			>
				<option value="">Pick env…</option>
				{#each envs as e (e.id)}
					<option value={e.id}>{e.name}</option>
				{/each}
			</select>
			<select
				bind:value={selProfile}
				class="h-8 rounded-md border border-border bg-background px-2 text-xs"
			>
				<option value="">Pick profile…</option>
				{#each profiles as p (p.id)}
					<option value={p.id}>{p.name}</option>
				{/each}
			</select>
			<select
				bind:value={selStrategy}
				class="h-8 rounded-md border border-border bg-background px-2 text-xs"
			>
				{#each strategies as s (s.id)}
					<option value={s.id}>{s.name}</option>
				{/each}
			</select>
			<Input bind:value={configVersion} placeholder="version (e.g. 1)" class="h-8 text-xs" />
		</div>
		<Button size="sm" onclick={start} disabled={starting}>
			<PlayIcon />
			{starting ? 'Starting…' : 'Start deployment'}
		</Button>
	</div>

	<DataTable
		{rows}
		{loading}
		columns={[
			{ key: 'deploymentNumber', label: '#', width: '60px', mono: true },
			{ key: 'configurationProfileId', label: 'Profile', mono: true },
			{ key: 'configurationVersion', label: 'Version', width: '100px', mono: true },
			{ key: 'deploymentStrategyId', label: 'Strategy', mono: true },
			{ key: 'state', label: 'State', width: '110px', cell: stateCell },
			{ key: 'percentageComplete', label: '%', width: '60px' }
		]}
		rowKey={(r) => `${r.environmentId}|${r.deploymentNumber}`}
	>
		{#snippet empty()}
			<EmptyState
				icon={RocketIcon}
				title="No deployments"
				description="Start a deployment from this environment by picking a profile and strategy."
			/>
		{/snippet}
	</DataTable>
</div>

{#snippet stateCell(row: Deployment)}
	<Badge variant="outline" class={`h-5 px-2 text-[10px] ${stateColor(row.state)}`}>
		{row.state}
	</Badge>
{/snippet}
