<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import UserCheckIcon from '@lucide/svelte/icons/user-check';
	import { toast } from 'svelte-sonner';
	import { getCallerIdentity, type CallerIdentity } from '$lib/api/sts';

	let identity = $state<CallerIdentity | null>(null);
	let loading = $state(false);

	async function load() {
		loading = true;
		try {
			identity = await getCallerIdentity();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'GetCallerIdentity failed');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});
</script>

<section class="flex flex-col gap-3 rounded-md border border-border bg-card/40 p-4">
	<header class="flex items-center justify-between">
		<div class="flex items-center gap-2">
			<UserCheckIcon class="size-4 text-muted-foreground" />
			<h2 class="text-sm font-semibold">Caller identity</h2>
		</div>
		<Button variant="ghost" size="xs" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</header>

	{#if loading && !identity}
		<div class="flex flex-col gap-2">
			<Skeleton class="h-4 w-32" />
			<Skeleton class="h-3 w-2/3" />
			<Skeleton class="h-3 w-1/2" />
		</div>
	{:else if identity}
		<dl class="grid grid-cols-3 gap-x-3 gap-y-2 text-xs">
			<dt class="text-muted-foreground">Account</dt>
			<dd class="col-span-2 font-mono text-base text-primary">{identity.account}</dd>
			<dt class="text-muted-foreground">ARN</dt>
			<dd class="col-span-2 break-all font-mono">{identity.arn}</dd>
			<dt class="text-muted-foreground">User ID</dt>
			<dd class="col-span-2 font-mono">{identity.userId}</dd>
		</dl>
	{:else}
		<p class="text-xs text-muted-foreground">No identity available.</p>
	{/if}
</section>
