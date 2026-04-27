<script lang="ts">
	import type { Instance } from '$lib/api/ec2';
	import { tagName } from '$lib/api/ec2';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';

	interface Props {
		instance: Instance | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { instance, open, onOpenChange }: Props = $props();

	function formatDate(iso?: string): string {
		if (!iso) return '—';
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}

	function stateVariant(state: string): 'default' | 'secondary' | 'destructive' | 'outline' {
		if (state === 'running') return 'default';
		if (state === 'stopped' || state === 'terminated') return 'destructive';
		if (state === 'pending' || state === 'stopping') return 'secondary';
		return 'outline';
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full overflow-y-auto sm:max-w-xl">
		{#if instance}
			<SheetHeader>
				<SheetTitle class="font-mono text-base">{instance.instanceId}</SheetTitle>
				<SheetDescription>
					{tagName(instance.tags) || 'Unnamed instance'} · {instance.instanceType}
				</SheetDescription>
			</SheetHeader>

			<div class="flex flex-col gap-4 p-4">
				<div class="grid grid-cols-2 gap-3 rounded-md border border-border bg-card p-3 text-sm">
					<div>
						<div class="text-xs text-muted-foreground">State</div>
						<div class="mt-0.5"><Badge variant={stateVariant(instance.state)}>{instance.state}</Badge></div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Type</div>
						<div class="mt-0.5 font-mono text-xs">{instance.instanceType}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">AMI</div>
						<div class="mt-0.5 font-mono text-xs">{instance.imageId || '—'}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Architecture</div>
						<div class="mt-0.5 font-mono text-xs">{instance.architecture || '—'}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Private IP</div>
						<div class="mt-0.5 font-mono text-xs">{instance.privateIp || '—'}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Public IP</div>
						<div class="mt-0.5 font-mono text-xs">{instance.publicIp || '—'}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">VPC</div>
						<div class="mt-0.5 font-mono text-xs">{instance.vpcId || '—'}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Subnet</div>
						<div class="mt-0.5 font-mono text-xs">{instance.subnetId || '—'}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Availability zone</div>
						<div class="mt-0.5 font-mono text-xs">{instance.availabilityZone || '—'}</div>
					</div>
					<div>
						<div class="text-xs text-muted-foreground">Key pair</div>
						<div class="mt-0.5 font-mono text-xs">{instance.keyName || '—'}</div>
					</div>
					<div class="col-span-2">
						<div class="text-xs text-muted-foreground">Launched</div>
						<div class="mt-0.5 font-mono text-xs">{formatDate(instance.launchTime)}</div>
					</div>
				</div>

				<section class="rounded-md border border-border bg-card">
					<header class="border-b border-border px-4 py-2">
						<h3 class="text-sm font-medium">Security groups ({instance.securityGroupIds.length})</h3>
					</header>
					{#if instance.securityGroupIds.length === 0}
						<p class="px-4 py-3 text-xs text-muted-foreground">No security groups attached.</p>
					{:else}
						<ul class="divide-y divide-border/40">
							{#each instance.securityGroupIds as sgId (sgId)}
								<li class="px-4 py-2 font-mono text-xs">{sgId}</li>
							{/each}
						</ul>
					{/if}
				</section>

				<section class="rounded-md border border-border bg-card">
					<header class="border-b border-border px-4 py-2">
						<h3 class="text-sm font-medium">Tags ({Object.keys(instance.tags).length})</h3>
					</header>
					{#if Object.keys(instance.tags).length === 0}
						<p class="px-4 py-3 text-xs text-muted-foreground">No tags.</p>
					{:else}
						<ul class="divide-y divide-border/40">
							{#each Object.entries(instance.tags) as [k, v] (k)}
								<li class="flex items-center justify-between gap-3 px-4 py-2">
									<span class="font-mono text-xs">{k}</span>
									<span class="font-mono text-xs text-muted-foreground">{v}</span>
								</li>
							{/each}
						</ul>
					{/if}
				</section>
			</div>
		{/if}
	</SheetContent>
</Sheet>
