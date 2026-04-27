<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import UsersRoundIcon from '@lucide/svelte/icons/users-round';
	import { toast } from 'svelte-sonner';
	import { getFederationToken, type Credentials } from '$lib/api/sts';
	import SessionCredentialsDisplay from './session-credentials-display.svelte';

	let name = $state('federated-user');
	let durationSeconds = $state(3600);
	let policy = $state('');
	let federatedUserArn = $state<string | null>(null);
	let working = $state(false);
	let credentials = $state<Credentials | null>(null);

	async function submit() {
		if (!name.trim()) {
			toast.error('Name is required.');
			return;
		}
		working = true;
		credentials = null;
		federatedUserArn = null;
		try {
			const r = await getFederationToken({
				name: name.trim(),
				durationSeconds,
				policy: policy.trim() || undefined
			});
			credentials = r.credentials;
			federatedUserArn = r.federatedUser.arn;
			toast.success('GetFederationToken succeeded.');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'GetFederationToken failed');
		} finally {
			working = false;
		}
	}
</script>

<section class="flex flex-col gap-3 rounded-md border border-border bg-card/40 p-4">
	<header class="flex items-center gap-2">
		<UsersRoundIcon class="size-4 text-muted-foreground" />
		<h2 class="text-sm font-semibold">Get federation token</h2>
	</header>

	<div class="grid gap-3 sm:grid-cols-2">
		<div class="flex flex-col gap-1">
			<Label for="sts-fed-name">Name</Label>
			<Input id="sts-fed-name" bind:value={name} />
		</div>
		<div class="flex flex-col gap-1">
			<Label for="sts-fed-duration">Duration (s)</Label>
			<Input id="sts-fed-duration" type="number" bind:value={durationSeconds} min={900} />
		</div>
		<div class="flex flex-col gap-1 sm:col-span-2">
			<Label for="sts-fed-policy">Policy (JSON)</Label>
			<Textarea
				id="sts-fed-policy"
				bind:value={policy}
				rows={4}
				class="font-mono text-xs"
				placeholder={'{"Version":"2012-10-17","Statement":[]}'}
			/>
		</div>
	</div>

	<div class="flex justify-end">
		<Button onclick={submit} disabled={working || !name.trim()}>
			{working ? 'Working…' : 'GetFederationToken'}
		</Button>
	</div>

	{#if federatedUserArn}
		<p class="text-xs text-muted-foreground">
			Federated user: <span class="font-mono">{federatedUserArn}</span>
		</p>
	{/if}

	<SessionCredentialsDisplay {credentials} />
</section>
