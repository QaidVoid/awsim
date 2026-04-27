<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import { toast } from 'svelte-sonner';
	import { assumeRole, type Credentials } from '$lib/api/sts';
	import SessionCredentialsDisplay from './session-credentials-display.svelte';

	let roleArn = $state('arn:aws:iam::000000000000:role/my-role');
	let roleSessionName = $state('my-session');
	let durationSeconds = $state(3600);
	let externalId = $state('');
	let policy = $state('');
	let assuming = $state(false);
	let credentials = $state<Credentials | null>(null);

	async function submit() {
		if (!roleArn.trim() || !roleSessionName.trim()) {
			toast.error('Role ARN and session name are required.');
			return;
		}
		assuming = true;
		credentials = null;
		try {
			const r = await assumeRole({
				roleArn: roleArn.trim(),
				roleSessionName: roleSessionName.trim(),
				durationSeconds,
				externalId: externalId.trim() || undefined,
				policy: policy.trim() || undefined
			});
			credentials = r.credentials;
			toast.success('AssumeRole succeeded.');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'AssumeRole failed');
		} finally {
			assuming = false;
		}
	}
</script>

<section class="flex flex-col gap-3 rounded-md border border-border bg-card/40 p-4">
	<header class="flex items-center gap-2">
		<KeyRoundIcon class="size-4 text-muted-foreground" />
		<h2 class="text-sm font-semibold">Assume role</h2>
	</header>

	<div class="grid gap-3 sm:grid-cols-2">
		<div class="flex flex-col gap-1 sm:col-span-2">
			<Label for="sts-role-arn">Role ARN</Label>
			<Input
				id="sts-role-arn"
				bind:value={roleArn}
				placeholder="arn:aws:iam::123456789012:role/MyRole"
				class="font-mono text-xs"
			/>
		</div>
		<div class="flex flex-col gap-1">
			<Label for="sts-role-session">Session name</Label>
			<Input id="sts-role-session" bind:value={roleSessionName} placeholder="my-session" />
		</div>
		<div class="flex flex-col gap-1">
			<Label for="sts-role-duration">Duration (s)</Label>
			<Input id="sts-role-duration" type="number" bind:value={durationSeconds} min={900} />
		</div>
		<div class="flex flex-col gap-1">
			<Label for="sts-role-external">External ID (optional)</Label>
			<Input id="sts-role-external" bind:value={externalId} />
		</div>
		<div class="flex flex-col gap-1 sm:col-span-2">
			<Label for="sts-role-policy">Inline policy (optional JSON)</Label>
			<Textarea
				id="sts-role-policy"
				bind:value={policy}
				rows={4}
				class="font-mono text-xs"
				placeholder={'{"Version":"2012-10-17","Statement":[]}'}
			/>
		</div>
	</div>

	<div class="flex justify-end">
		<Button onclick={submit} disabled={assuming || !roleArn.trim() || !roleSessionName.trim()}>
			{assuming ? 'Assuming…' : 'AssumeRole'}
		</Button>
	</div>

	<SessionCredentialsDisplay {credentials} />
</section>
