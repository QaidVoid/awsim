<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { describeUserPool, updateUserPool } from '$lib/api/cognito';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		poolId: string;
	}

	let { poolId }: Props = $props();

	/// Cognito's published trigger surface — order mirrors the AWS console.
	const TRIGGERS: { key: string; label: string; description: string }[] = [
		{
			key: 'PreSignUp',
			label: 'Pre sign-up',
			description: 'Auto-confirm or auto-verify a user, or veto a sign-up.'
		},
		{
			key: 'PostConfirmation',
			label: 'Post confirmation',
			description: 'Fires after a user finishes sign-up confirmation.'
		},
		{
			key: 'PreAuthentication',
			label: 'Pre authentication',
			description: 'Custom validation immediately before sign-in.'
		},
		{
			key: 'PostAuthentication',
			label: 'Post authentication',
			description: 'Track a successful sign-in event.'
		},
		{
			key: 'CustomMessage',
			label: 'Custom message',
			description: 'Override invitation / verification email & SMS bodies.'
		},
		{
			key: 'DefineAuthChallenge',
			label: 'Define auth challenge',
			description: 'Drive custom challenge sequence for adaptive auth.'
		},
		{
			key: 'CreateAuthChallenge',
			label: 'Create auth challenge',
			description: 'Generate the next challenge to send to the client.'
		},
		{
			key: 'VerifyAuthChallengeResponse',
			label: 'Verify auth challenge response',
			description: 'Decide whether the user answered the challenge correctly.'
		},
		{
			key: 'PreTokenGeneration',
			label: 'Pre token generation',
			description: 'Mutate token claims before issuance.'
		},
		{
			key: 'UserMigration',
			label: 'User migration',
			description: 'Sign-in-time import from a legacy user store.'
		},
		{
			key: 'CustomEmailSender',
			label: 'Custom email sender',
			description: 'Replace the default SES sender with a Lambda.'
		},
		{
			key: 'CustomSMSSender',
			label: 'Custom SMS sender',
			description: 'Replace the default SNS sender with a Lambda.'
		}
	];

	let original = $state<Record<string, string>>({});
	let arns = $state<Record<string, string>>({});
	let loading = $state(true);
	let saving = $state(false);

	const dirty = $derived.by(() => {
		const keys = new Set([...Object.keys(original), ...Object.keys(arns)]);
		for (const k of keys) {
			if ((original[k] ?? '') !== (arns[k] ?? '').trim()) return true;
		}
		return false;
	});

	onMount(load);

	async function load() {
		loading = true;
		try {
			const detail = await describeUserPool(poolId);
			const cfg: Record<string, string> = {};
			for (const [k, v] of Object.entries(detail.lambdaConfig ?? {})) {
				if (typeof v === 'string') cfg[k] = v;
			}
			original = cfg;
			// Initialize the editable buffer with current values for each
			// known trigger; unknown extra keys are still preserved on save
			// so we don't silently drop config we don't recognise.
			arns = { ...cfg };
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load triggers');
		} finally {
			loading = false;
		}
	}

	async function save() {
		saving = true;
		try {
			const next: Record<string, string> = {};
			// Preserve any keys the UI doesn't show.
			for (const [k, v] of Object.entries(original)) {
				if (!TRIGGERS.find((t) => t.key === k) && v) next[k] = v;
			}
			for (const t of TRIGGERS) {
				const v = (arns[t.key] ?? '').trim();
				if (v) next[t.key] = v;
			}
			await updateUserPool(poolId, { lambdaConfig: next });
			toast.success('Lambda triggers saved');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Save failed');
		} finally {
			saving = false;
		}
	}

	function reset() {
		arns = { ...original };
	}
</script>

<div class="space-y-4">
	{#if loading}
		<p class="text-xs text-muted-foreground">
			<Loader2 class="inline size-3 animate-spin" /> Loading triggers...
		</p>
	{:else}
		<p class="text-xs text-muted-foreground">
			Map a Cognito trigger to a Lambda function ARN. Leave blank to disable. ARNs aren't
			validated; awsim's Lambda emulator must own the function ARN for the trigger to fire.
		</p>
		<div class="space-y-2">
			{#each TRIGGERS as t (t.key)}
				<div class="rounded border border-border/60 px-3 py-2">
					<div class="mb-1 flex items-baseline justify-between gap-2">
						<Label for={`trigger-${t.key}`} class="font-medium">{t.label}</Label>
						<code class="font-mono text-[10px] text-muted-foreground">{t.key}</code>
					</div>
					<p class="mb-2 text-xs text-muted-foreground">{t.description}</p>
					<Input
						id={`trigger-${t.key}`}
						bind:value={arns[t.key]}
						placeholder="arn:aws:lambda:us-east-1:000000000000:function:my-fn"
						class="h-8 font-mono text-xs"
						autocomplete="off"
					/>
				</div>
			{/each}
		</div>
		<div class="sticky bottom-0 -mx-3 flex items-center justify-end gap-2 border-t border-border bg-background px-3 py-2">
			<Button variant="ghost" size="sm" onclick={reset} disabled={saving || !dirty}>
				Discard
			</Button>
			<Button size="sm" onclick={save} disabled={saving || !dirty}>
				{#if saving}<Loader2 class="size-3.5 animate-spin" />{/if}
				Save changes
			</Button>
		</div>
	{/if}
</div>
