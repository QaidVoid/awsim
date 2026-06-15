<script lang="ts">
	import { toast } from 'svelte-sonner';
	import {
		createClusterInstance,
		failoverDBCluster,
		formatTimestamp,
		statusVariant,
		type DBCluster
	} from '$lib/api/rds';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { EmptyState } from '$lib/components/service';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Plus from '@lucide/svelte/icons/plus';
	import Server from '@lucide/svelte/icons/server';
	import ArrowUpCircle from '@lucide/svelte/icons/arrow-up-circle';

	interface Props {
		cluster: DBCluster;
		onDeleteCluster: (cluster: DBCluster) => void;
		onChanged: () => Promise<void>;
	}

	let { cluster, onDeleteCluster, onChanged }: Props = $props();

	let newInstanceId = $state('');
	let newInstanceClass = $state('db.r6g.large');
	let addingInstance = $state(false);
	let failingOver = $state<string | null>(null);

	async function addInstance() {
		const id = newInstanceId.trim();
		if (!id) {
			toast.error('Instance identifier required');
			return;
		}
		addingInstance = true;
		try {
			await createClusterInstance({
				identifier: id,
				clusterIdentifier: cluster.identifier,
				engine: cluster.engine,
				instanceClass: newInstanceClass
			});
			toast.success(`Added instance ${id}`);
			newInstanceId = '';
			await onChanged();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to add instance');
		} finally {
			addingInstance = false;
		}
	}

	async function promote(instanceId: string) {
		failingOver = instanceId;
		try {
			await failoverDBCluster(cluster.identifier, instanceId);
			toast.success(`Promoted ${instanceId} to writer`);
			await onChanged();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failover failed');
		} finally {
			failingOver = null;
		}
	}
</script>

<div class="border-t border-border bg-muted/20 px-4 py-4">
	<div class="grid gap-6 lg:grid-cols-2">
		<dl class="grid h-fit grid-cols-[130px_1fr] gap-y-2 text-xs">
			<dt class="text-muted-foreground">Status</dt>
			<dd>
				<Badge variant={statusVariant(cluster.status)}>{cluster.status || 'unknown'}</Badge>
			</dd>

			<dt class="text-muted-foreground">Engine</dt>
			<dd class="font-mono">{cluster.engine} {cluster.engineVersion ?? ''}</dd>

			<dt class="text-muted-foreground">Engine mode</dt>
			<dd class="font-mono">{cluster.engineMode || '—'}</dd>

			<dt class="text-muted-foreground">Writer endpoint</dt>
			<dd class="font-mono break-all">
				{cluster.endpoint || '—'}{cluster.port ? `:${cluster.port}` : ''}
			</dd>

			<dt class="text-muted-foreground">Reader endpoint</dt>
			<dd class="font-mono break-all">
				{cluster.readerEndpoint || '—'}{cluster.port ? `:${cluster.port}` : ''}
			</dd>

			<dt class="text-muted-foreground">Master user</dt>
			<dd class="font-mono">{cluster.masterUsername || '—'}</dd>

			{#if cluster.serverlessMinCapacity != null && cluster.serverlessMaxCapacity != null}
				<dt class="text-muted-foreground">Serverless v2</dt>
				<dd class="font-mono">
					{cluster.serverlessMinCapacity}-{cluster.serverlessMaxCapacity} ACU
				</dd>
			{/if}

			<dt class="text-muted-foreground">Deletion protection</dt>
			<dd>{cluster.deletionProtection ? 'Enabled' : 'Disabled'}</dd>

			<dt class="text-muted-foreground">Data API (HTTP)</dt>
			<dd>{cluster.httpEndpointEnabled ? 'Enabled' : 'Disabled'}</dd>

			{#if cluster.arn}
				<dt class="text-muted-foreground">ARN</dt>
				<dd class="font-mono text-[11px] break-all">{cluster.arn}</dd>
			{/if}
		</dl>

		<div class="flex flex-col gap-2">
			<span class="text-xs font-medium text-muted-foreground">
				Instances ({cluster.members.length})
			</span>
			<div class="flex items-end gap-2">
				<Input
					bind:value={newInstanceId}
					placeholder="instance identifier"
					class="h-8 flex-1 text-xs"
				/>
				<Select type="single" bind:value={newInstanceClass}>
					<SelectTrigger class="h-8 w-36 text-xs">{newInstanceClass}</SelectTrigger>
					<SelectContent>
						<SelectItem value="db.r6g.large" label="db.r6g.large">db.r6g.large</SelectItem>
						<SelectItem value="db.r5.large" label="db.r5.large">db.r5.large</SelectItem>
						<SelectItem value="db.serverless" label="db.serverless">db.serverless</SelectItem>
						<SelectItem value="db.t3.medium" label="db.t3.medium">db.t3.medium</SelectItem>
					</SelectContent>
				</Select>
				<Button size="sm" onclick={addInstance} disabled={addingInstance}>
					{#if addingInstance}
						<Loader2 class="size-3.5 animate-spin" />
					{:else}
						<Plus class="size-3.5" />
					{/if}
					Add
				</Button>
			</div>

			{#if cluster.members.length === 0}
				<EmptyState
					icon={Server}
					title="No instances"
					description="Add an instance to give the cluster a writer."
				/>
			{:else}
				<table class="w-full text-xs">
					<thead>
						<tr class="border-b border-border text-left text-muted-foreground">
							<th class="py-1.5 pr-2 font-medium">Instance</th>
							<th class="py-1.5 pr-2 font-medium">Role</th>
							<th></th>
						</tr>
					</thead>
					<tbody>
						{#each cluster.members as member (member.instanceId)}
							<tr class="border-b border-border/30">
								<td class="py-1.5 pr-2 font-mono break-all">{member.instanceId}</td>
								<td class="py-1.5 pr-2">
									<Badge variant={member.isWriter ? 'secondary' : 'outline'}>
										{member.isWriter ? 'Writer' : 'Reader'}
									</Badge>
								</td>
								<td class="py-1.5 pl-1 text-right">
									{#if !member.isWriter}
										<Button
											variant="ghost"
											size="icon-xs"
											aria-label="Promote to writer"
											title="Promote to writer (failover)"
											disabled={failingOver !== null}
											onclick={() => promote(member.instanceId)}
										>
											{#if failingOver === member.instanceId}
												<Loader2 class="size-3 animate-spin" />
											{:else}
												<ArrowUpCircle class="size-3.5" />
											{/if}
										</Button>
									{/if}
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			{/if}
		</div>
	</div>

	<div
		class="mt-4 flex items-center justify-between border-t border-border/50 pt-3"
	>
		<span class="text-[11px] text-muted-foreground">
			{#if cluster.createdAt}Created {formatTimestamp(cluster.createdAt)}{/if}
		</span>
		<Button variant="destructive" size="sm" onclick={() => onDeleteCluster(cluster)}>
			<Trash2 class="size-3.5" />
			Delete cluster
		</Button>
	</div>
</div>
