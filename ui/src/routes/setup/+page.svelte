<script lang="ts">
	import { goto } from '$app/navigation';
	import { setup, type AuthError, type SetupResponse } from '$lib/api/auth';
	import { route } from '$lib/url';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Card,
		CardContent,
		CardDescription,
		CardFooter,
		CardHeader,
		CardTitle
	} from '$lib/components/ui/card';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import AlertCircle from '@lucide/svelte/icons/alert-circle';
	import Copy from '@lucide/svelte/icons/copy';
	import { toast } from 'svelte-sonner';

	let bootstrapToken = $state('');
	let password = $state('');
	let confirmPassword = $state('');
	let submitting = $state(false);
	let errorMessage = $state<string | null>(null);
	let result = $state<SetupResponse | null>(null);

	const passwordMismatch = $derived(
		confirmPassword.length > 0 && password !== confirmPassword
	);

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		errorMessage = null;

		if (passwordMismatch) {
			errorMessage = 'Passwords do not match.';
			return;
		}

		submitting = true;
		try {
			result = await setup({
				bootstrap_token: bootstrapToken.trim(),
				password
			});
		} catch (err) {
			const e = err as AuthError;
			errorMessage = e.message || 'Setup failed.';
		} finally {
			submitting = false;
		}
	}

	async function copy(value: string, label: string) {
		try {
			await navigator.clipboard.writeText(value);
			toast.success(`${label} copied.`);
		} catch {
			toast.error('Clipboard unavailable.');
		}
	}
</script>

<svelte:head>
	<title>First-run setup - AWSim</title>
</svelte:head>

<div class="flex min-h-screen w-full items-center justify-center bg-muted/30 px-4 py-10">
	<div class="flex w-full max-w-lg flex-col gap-6">
		<div class="flex flex-col items-center gap-2 text-center">
			<div
				class="flex h-11 w-11 items-center justify-center rounded-md bg-primary text-primary-foreground"
			>
				<ShieldCheck class="size-5" />
			</div>
			<h1 class="text-xl font-semibold tracking-tight">First-run setup</h1>
			<p class="text-sm text-muted-foreground">
				Create the root operator. This page runs once per data directory.
			</p>
		</div>

		{#if result}
			<Card>
				<CardHeader>
					<CardTitle>Setup complete</CardTitle>
					<CardDescription>
						Save these credentials now. The secret access key cannot be retrieved again.
					</CardDescription>
				</CardHeader>
				<CardContent class="flex flex-col gap-3">
					<div class="flex flex-col gap-1.5">
						<Label class="text-[11px] uppercase text-muted-foreground">Principal</Label>
						<div class="flex items-center gap-2">
							<code class="flex-1 truncate rounded-md border bg-muted/40 px-2 py-1.5 font-mono text-xs"
								>{result.principal}</code
							>
							<Button
								variant="outline"
								size="icon-xs"
								onclick={() => copy(result!.principal, 'Principal')}
							>
								<Copy />
							</Button>
						</div>
					</div>
					<div class="flex flex-col gap-1.5">
						<Label class="text-[11px] uppercase text-muted-foreground">Access key ID</Label>
						<div class="flex items-center gap-2">
							<code class="flex-1 truncate rounded-md border bg-muted/40 px-2 py-1.5 font-mono text-xs"
								>{result.access_key_id}</code
							>
							<Button
								variant="outline"
								size="icon-xs"
								onclick={() => copy(result!.access_key_id, 'Access key ID')}
							>
								<Copy />
							</Button>
						</div>
					</div>
					<div class="flex flex-col gap-1.5">
						<Label class="text-[11px] uppercase text-muted-foreground">Secret access key</Label>
						<div class="flex items-center gap-2">
							<code
								class="flex-1 truncate rounded-md border bg-destructive/5 px-2 py-1.5 font-mono text-xs"
								>{result.secret_access_key}</code
							>
							<Button
								variant="outline"
								size="icon-xs"
								onclick={() => copy(result!.secret_access_key, 'Secret access key')}
							>
								<Copy />
							</Button>
						</div>
					</div>
				</CardContent>
				<CardFooter>
					<Button class="w-full" onclick={() => goto(route('/login'))}>
						Continue to sign in
					</Button>
				</CardFooter>
			</Card>
		{:else}
			<Card>
				<CardHeader>
					<CardTitle>Create root operator</CardTitle>
					<CardDescription>
						Paste the bootstrap token printed to AWSim's stdout on first boot, then pick
						a strong root password.
					</CardDescription>
				</CardHeader>
				<CardContent>
					<form onsubmit={handleSubmit} class="flex flex-col gap-3">
						<div class="flex flex-col gap-1.5">
							<Label for="setup-token">Bootstrap token</Label>
							<Input
								id="setup-token"
								bind:value={bootstrapToken}
								required
								autocomplete="off"
								autocapitalize="off"
								autocorrect="off"
								spellcheck="false"
								class="font-mono text-xs"
								placeholder="64-character hex token"
							/>
						</div>
						<div class="flex flex-col gap-1.5">
							<Label for="setup-pass">Root password</Label>
							<Input
								id="setup-pass"
								type="password"
								bind:value={password}
								required
								minlength={8}
								autocomplete="new-password"
							/>
							<p class="text-[11px] text-muted-foreground">Minimum 8 characters.</p>
						</div>
						<div class="flex flex-col gap-1.5">
							<Label for="setup-confirm">Confirm password</Label>
							<Input
								id="setup-confirm"
								type="password"
								bind:value={confirmPassword}
								required
								minlength={8}
								autocomplete="new-password"
								aria-invalid={passwordMismatch ? 'true' : undefined}
							/>
							{#if passwordMismatch}
								<p class="text-[11px] text-destructive">Passwords do not match.</p>
							{/if}
						</div>

						{#if errorMessage}
							<Alert variant="destructive">
								<AlertCircle />
								<AlertTitle>Setup failed</AlertTitle>
								<AlertDescription>{errorMessage}</AlertDescription>
							</Alert>
						{/if}

						<Button
							type="submit"
							class="mt-1"
							disabled={submitting ||
								!bootstrapToken.trim() ||
								!password ||
								passwordMismatch}
						>
							{#if submitting}
								<Loader2 class="size-3.5 animate-spin" />
							{:else}
								<KeyRound class="size-3.5" />
							{/if}
							Create root operator
						</Button>
					</form>
				</CardContent>
			</Card>
		{/if}
	</div>
</div>
