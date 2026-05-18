<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		listAppClients,
		adminInitiateAuth,
		adminRespondToAuthChallenge,
		adminRefreshTokens,
		type AuthTokens,
		type AuthChallenge
	} from '$lib/api/cognito';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import LogIn from '@lucide/svelte/icons/log-in';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Copy from '@lucide/svelte/icons/copy';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import ScanSearch from '@lucide/svelte/icons/scan-search';
	import JwtDecoder from './jwt-decoder.svelte';

	interface Props {
		poolId: string;
		/** Optional username to seed the form with (e.g. from a user row). */
		prefillUser?: string;
	}

	let { poolId, prefillUser = '' }: Props = $props();

	let clients = $state<{ clientId: string; clientName: string }[]>([]);
	let clientsLoading = $state(true);
	let clientId = $state('');
	let username = $state('');
	let password = $state('');
	let submitting = $state(false);

	let challenge = $state<AuthChallenge | null>(null);
	let tokens = $state<AuthTokens | null>(null);
	let newPassword = $state('');
	let mfaCode = $state('');

	let decoderToken = $state('');

	const selectedClientName = $derived(
		clients.find((c) => c.clientId === clientId)?.clientName ?? 'Select an app client'
	);

	$effect(() => {
		if (prefillUser && !username) username = prefillUser;
	});

	onMount(async () => {
		try {
			const page = await listAppClients(poolId);
			clients = page.clients;
			if (clients.length === 1) clientId = clients[0].clientId;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load app clients');
		} finally {
			clientsLoading = false;
		}
	});

	function applyOutcome(
		outcome: { kind: 'tokens'; tokens: AuthTokens } | { kind: 'challenge'; challenge: AuthChallenge }
	) {
		if (outcome.kind === 'tokens') {
			tokens = outcome.tokens;
			challenge = null;
			decoderToken = outcome.tokens.idToken;
			toast.success('Signed in');
		} else {
			challenge = outcome.challenge;
			tokens = null;
			newPassword = '';
			mfaCode = '';
		}
	}

	async function signIn() {
		if (!clientId) {
			toast.error('Pick an app client first.');
			return;
		}
		if (!username.trim() || !password) {
			toast.error('Username and password are required.');
			return;
		}
		submitting = true;
		try {
			applyOutcome(
				await adminInitiateAuth({
					poolId,
					clientId,
					username: username.trim(),
					password
				})
			);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Sign-in failed');
		} finally {
			submitting = false;
		}
	}

	async function respond() {
		if (!challenge) return;
		const responses: Record<string, string> = { USERNAME: username.trim() };
		if (challenge.challengeName === 'NEW_PASSWORD_REQUIRED') {
			if (!newPassword) {
				toast.error('Enter a new password.');
				return;
			}
			responses.NEW_PASSWORD = newPassword;
		} else if (challenge.challengeName === 'SOFTWARE_TOKEN_MFA') {
			if (!mfaCode.trim()) {
				toast.error('Enter the 6-digit code.');
				return;
			}
			responses.SOFTWARE_TOKEN_MFA_CODE = mfaCode.trim();
		}
		submitting = true;
		try {
			applyOutcome(
				await adminRespondToAuthChallenge({
					poolId,
					clientId,
					challengeName: challenge.challengeName,
					session: challenge.session,
					responses
				})
			);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Challenge response failed');
		} finally {
			submitting = false;
		}
	}

	async function refresh() {
		if (!tokens?.refreshToken) return;
		submitting = true;
		try {
			const next = await adminRefreshTokens({
				poolId,
				clientId,
				refreshToken: tokens.refreshToken
			});
			tokens = { ...next, refreshToken: next.refreshToken ?? tokens.refreshToken };
			decoderToken = tokens.idToken;
			toast.success('Tokens refreshed');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Refresh failed');
		} finally {
			submitting = false;
		}
	}

	function reset() {
		tokens = null;
		challenge = null;
		password = '';
		newPassword = '';
		mfaCode = '';
		decoderToken = '';
	}

	async function copy(value: string) {
		try {
			await navigator.clipboard.writeText(value);
			toast.success('Copied');
		} catch {
			toast.error('Clipboard unavailable');
		}
	}
</script>

{#snippet tokenRow(label: string, value: string | undefined)}
	{#if value}
		<div class="flex items-center gap-2">
			<span class="w-16 shrink-0 text-xs font-medium text-muted-foreground">{label}</span>
			<code class="min-w-0 flex-1 truncate rounded border border-border/60 bg-muted/30 px-2 py-1 font-mono text-[11px]">
				{value}
			</code>
			<Button variant="ghost" size="icon-sm" title="Copy" onclick={() => copy(value)}>
				<Copy class="size-3.5" />
			</Button>
			<Button variant="ghost" size="icon-sm" title="Decode" onclick={() => (decoderToken = value)}>
				<ScanSearch class="size-3.5" />
			</Button>
		</div>
	{/if}
{/snippet}

<div class="w-full space-y-4 overflow-y-auto px-6 py-4">
	<Card>
		<CardHeader>
			<CardTitle class="flex items-center gap-2">
				<LogIn class="size-4 text-primary" /> Sign in
			</CardTitle>
		</CardHeader>
		<CardContent class="grid gap-4">
			<p class="text-xs text-muted-foreground">
				Authenticate a pool user with the admin password flow
				(ADMIN_USER_PASSWORD_AUTH) and inspect the issued tokens. Use an app
				client without a generated secret.
			</p>

			<div class="grid gap-3 sm:grid-cols-3">
				<div class="flex flex-col gap-1.5">
					<Label class="text-xs">App client</Label>
					<Select
						type="single"
						value={clientId}
						onValueChange={(v) => (clientId = v ?? '')}
						disabled={clientsLoading || submitting}
					>
						<SelectTrigger size="sm" class="text-xs">
							{clientsLoading ? 'Loading...' : selectedClientName}
						</SelectTrigger>
						<SelectContent>
							{#each clients as c (c.clientId)}
								<SelectItem value={c.clientId} label={c.clientName}>
									{c.clientName}
									<span class="ml-1 font-mono text-[10px] text-muted-foreground">{c.clientId}</span>
								</SelectItem>
							{/each}
						</SelectContent>
					</Select>
				</div>
				<div class="flex flex-col gap-1.5">
					<Label for="auth-username" class="text-xs">Username</Label>
					<Input
						id="auth-username"
						bind:value={username}
						class="h-8 text-xs"
						disabled={submitting}
						placeholder="jane or jane@example.com"
					/>
				</div>
				<div class="flex flex-col gap-1.5">
					<Label for="auth-password" class="text-xs">Password</Label>
					<Input
						id="auth-password"
						type="password"
						bind:value={password}
						class="h-8 text-xs"
						disabled={submitting}
						onkeydown={(e) => e.key === 'Enter' && signIn()}
					/>
				</div>
			</div>

			<div class="flex items-center gap-2">
				<Button size="sm" onclick={signIn} disabled={submitting || clientsLoading}>
					{#if submitting && !challenge}
						<Loader2 class="size-3.5 animate-spin" />
					{:else}
						<LogIn class="size-3.5" />
					{/if}
					Sign in
				</Button>
				{#if tokens || challenge}
					<Button variant="ghost" size="sm" onclick={reset} disabled={submitting}>Reset</Button>
				{/if}
				{#if clients.length === 0 && !clientsLoading}
					<span class="text-xs text-destructive">
						No app clients in this pool. Create one under "App clients" first.
					</span>
				{/if}
			</div>

			{#if challenge}
				<div class="space-y-3 rounded border border-amber-500/40 bg-amber-500/5 p-3">
					<div class="flex items-center gap-2 text-xs">
						<Badge variant="outline">challenge</Badge>
						<span class="font-mono">{challenge.challengeName}</span>
					</div>
					{#if challenge.challengeName === 'NEW_PASSWORD_REQUIRED'}
						<p class="text-xs text-muted-foreground">
							This user must set a permanent password before tokens are issued.
						</p>
						<div class="flex items-end gap-2">
							<div class="flex flex-1 flex-col gap-1.5">
								<Label for="auth-newpw" class="text-xs">New password</Label>
								<Input
									id="auth-newpw"
									type="password"
									bind:value={newPassword}
									class="h-8 text-xs"
									disabled={submitting}
									onkeydown={(e) => e.key === 'Enter' && respond()}
								/>
							</div>
							<Button size="sm" onclick={respond} disabled={submitting}>
								{#if submitting}<Loader2 class="size-3.5 animate-spin" />{/if}
								Set & continue
							</Button>
						</div>
					{:else if challenge.challengeName === 'SOFTWARE_TOKEN_MFA'}
						<p class="text-xs text-muted-foreground">
							Enter the current TOTP code from the authenticator app.
						</p>
						<div class="flex items-end gap-2">
							<div class="flex flex-col gap-1.5">
								<Label for="auth-mfa" class="text-xs">6-digit code</Label>
								<Input
									id="auth-mfa"
									bind:value={mfaCode}
									class="h-8 w-32 font-mono text-xs"
									disabled={submitting}
									placeholder="123456"
									onkeydown={(e) => e.key === 'Enter' && respond()}
								/>
							</div>
							<Button size="sm" onclick={respond} disabled={submitting}>
								{#if submitting}<Loader2 class="size-3.5 animate-spin" />{/if}
								Verify
							</Button>
						</div>
					{:else}
						<p class="text-xs text-muted-foreground">
							This challenge type is not interactively supported here. Session:
							<code class="font-mono">{challenge.session}</code>
						</p>
					{/if}
				</div>
			{/if}

			{#if tokens}
				<div class="space-y-2 rounded border border-border/60 p-3">
					<div class="flex items-center justify-between">
						<span class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
							Issued tokens
						</span>
						<div class="flex items-center gap-2">
							{#if tokens.expiresIn}
								<Badge variant="secondary" class="text-[10px]">
									expires in {tokens.expiresIn}s
								</Badge>
							{/if}
							{#if tokens.refreshToken}
								<Button variant="ghost" size="xs" onclick={refresh} disabled={submitting}>
									<RefreshCw class="size-3 {submitting ? 'animate-spin' : ''}" />
									Refresh
								</Button>
							{/if}
						</div>
					</div>
					{@render tokenRow('ID', tokens.idToken)}
					{@render tokenRow('Access', tokens.accessToken)}
					{@render tokenRow('Refresh', tokens.refreshToken)}
				</div>
			{/if}
		</CardContent>
	</Card>

	<JwtDecoder bind:token={decoderToken} />
</div>
