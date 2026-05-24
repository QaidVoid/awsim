<script lang="ts">
	import { goto } from '$app/navigation';
	import { login, type AuthError } from '$lib/api/auth';
	import { auth } from '$lib/auth-state.svelte';
	import { route } from '$lib/url';
	import { onMount } from 'svelte';
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
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import ShieldUser from '@lucide/svelte/icons/shield-user';
	import Users from '@lucide/svelte/icons/users';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import AlertCircle from '@lucide/svelte/icons/alert-circle';

	type Mode = 'root' | 'iam';

	let mode = $state<Mode>('root');

	let rootPassword = $state('');
	let rootMfa = $state('');

	let iamUsername = $state('');
	let iamPassword = $state('');
	let iamMfa = $state('');

	let submitting = $state(false);
	let errorMessage = $state<string | null>(null);

	onMount(() => {
		void auth.refresh();
	});

	async function signIn(username: string, password: string, mfa: string) {
		submitting = true;
		errorMessage = null;
		try {
			await login({
				username,
				password,
				mfa_code: mfa.trim() ? mfa.trim() : undefined
			});
			await auth.refresh();
			await goto(route('/'));
		} catch (err) {
			const e = err as AuthError;
			errorMessage = e.message || 'Sign-in failed.';
			if (e.retry_after) {
				errorMessage = `${errorMessage} Retry in ${e.retry_after}s.`;
			}
		} finally {
			submitting = false;
		}
	}

	function submitRoot(event: SubmitEvent) {
		event.preventDefault();
		void signIn('root', rootPassword, rootMfa);
	}

	function submitIam(event: SubmitEvent) {
		event.preventDefault();
		void signIn(iamUsername.trim(), iamPassword, iamMfa);
	}
</script>

<svelte:head>
	<title>Sign in - AWSim</title>
</svelte:head>

<div class="flex min-h-screen w-full items-center justify-center bg-muted/30 px-4 py-10">
	<div class="flex w-full max-w-md flex-col gap-6">
		<div class="flex flex-col items-center gap-2 text-center">
			<div
				class="flex h-11 w-11 items-center justify-center rounded-md bg-primary text-primary-foreground"
			>
				<span class="text-base font-semibold">A</span>
			</div>
			<h1 class="text-xl font-semibold tracking-tight">Sign in to AWSim</h1>
			<p class="text-sm text-muted-foreground">
				Choose how you want to sign in to the admin console.
			</p>
		</div>

		{#if auth.setupRequired}
			<Alert>
				<AlertCircle />
				<AlertTitle>First-run setup required</AlertTitle>
				<AlertDescription>
					No root operator exists yet. Run the bootstrap flow to create one before signing
					in.
					<Button
						class="mt-2 w-fit"
						variant="outline"
						size="sm"
						onclick={() => goto(route('/setup'))}
					>
						Go to setup
					</Button>
				</AlertDescription>
			</Alert>
		{:else}
			<Card>
				<Tabs value={mode} onValueChange={(v) => (mode = v as Mode)}>
					<CardHeader class="gap-3">
						<TabsList class="w-full">
							<TabsTrigger value="root" class="flex-1 gap-1.5">
								<ShieldUser class="size-3.5" /> Root user
							</TabsTrigger>
							<TabsTrigger value="iam" class="flex-1 gap-1.5">
								<Users class="size-3.5" /> IAM user
							</TabsTrigger>
						</TabsList>
						{#if mode === 'root'}
							<div>
								<CardTitle class="text-base">Root account</CardTitle>
								<CardDescription>
									The owner account created during first-run setup. Use this when you
									need full admin access or you've locked yourself out of IAM.
								</CardDescription>
							</div>
						{:else}
							<div>
								<CardTitle class="text-base">IAM user</CardTitle>
								<CardDescription>
									Day-to-day access using an IAM user with a console password.
								</CardDescription>
							</div>
						{/if}
					</CardHeader>

					<CardContent>
						<TabsContent value="root">
							<form onsubmit={submitRoot} class="flex flex-col gap-3">
								<div class="flex flex-col gap-1.5">
									<Label for="root-pass">Root password</Label>
									<Input
										id="root-pass"
										type="password"
										bind:value={rootPassword}
										required
										autocomplete="current-password"
									/>
								</div>
								<div class="flex flex-col gap-1.5">
									<Label for="root-mfa">MFA code (if enabled)</Label>
									<Input
										id="root-mfa"
										inputmode="numeric"
										autocomplete="one-time-code"
										placeholder="123456"
										bind:value={rootMfa}
										class="font-mono"
									/>
								</div>
								{#if errorMessage}
									<p class="flex items-center gap-1.5 text-xs text-destructive">
										<AlertCircle class="size-3.5" />
										{errorMessage}
									</p>
								{/if}
								<Button type="submit" class="mt-1" disabled={submitting || !rootPassword}>
									{#if submitting}<Loader2 class="size-3.5 animate-spin" />{:else}<KeyRound
											class="size-3.5"
										/>{/if}
									Sign in as root
								</Button>
							</form>
						</TabsContent>

						<TabsContent value="iam">
							<form onsubmit={submitIam} class="flex flex-col gap-3">
								<div class="flex flex-col gap-1.5">
									<Label for="iam-user">IAM user name</Label>
									<Input
										id="iam-user"
										bind:value={iamUsername}
										required
										autocomplete="username"
										autocapitalize="off"
										autocorrect="off"
										spellcheck="false"
									/>
								</div>
								<div class="flex flex-col gap-1.5">
									<Label for="iam-pass">Password</Label>
									<Input
										id="iam-pass"
										type="password"
										bind:value={iamPassword}
										required
										autocomplete="current-password"
									/>
								</div>
								<div class="flex flex-col gap-1.5">
									<Label for="iam-mfa">MFA code (if enabled)</Label>
									<Input
										id="iam-mfa"
										inputmode="numeric"
										autocomplete="one-time-code"
										placeholder="123456"
										bind:value={iamMfa}
										class="font-mono"
									/>
								</div>
								{#if errorMessage}
									<p class="flex items-center gap-1.5 text-xs text-destructive">
										<AlertCircle class="size-3.5" />
										{errorMessage}
									</p>
								{/if}
								<Button
									type="submit"
									class="mt-1"
									disabled={submitting || !iamUsername.trim() || !iamPassword}
								>
									{#if submitting}<Loader2 class="size-3.5 animate-spin" />{:else}<KeyRound
											class="size-3.5"
										/>{/if}
									Sign in
								</Button>
							</form>
						</TabsContent>
					</CardContent>

					<CardFooter class="flex justify-between border-t pt-4 text-[11px] text-muted-foreground">
						<span>Sessions last 12 hours.</span>
						<span>v0.4.1</span>
					</CardFooter>
				</Tabs>
			</Card>
		{/if}
	</div>
</div>
