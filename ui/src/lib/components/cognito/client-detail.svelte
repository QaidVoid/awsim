<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		describeAppClient,
		updateAppClient,
		type CognitoAppClientDetail,
		type SchemaAttribute
	} from '$lib/api/cognito';
	import AttributePermissions, {
		defaultClientPerms
	} from './attribute-permissions.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Eye from '@lucide/svelte/icons/eye';
	import EyeOff from '@lucide/svelte/icons/eye-off';
	import Copy from '@lucide/svelte/icons/copy';

	interface Props {
		poolId: string;
		clientId: string;
		schema: SchemaAttribute[];
	}

	let { poolId, clientId, schema }: Props = $props();

	const SCOPES = ['openid', 'email', 'phone', 'profile', 'aws.cognito.signin.user.admin'];
	const FLOWS = ['code', 'implicit', 'client_credentials'];
	const AUTH_FLOWS = [
		'ALLOW_USER_PASSWORD_AUTH',
		'ALLOW_USER_SRP_AUTH',
		'ALLOW_REFRESH_TOKEN_AUTH',
		'ALLOW_ADMIN_USER_PASSWORD_AUTH',
		'ALLOW_CUSTOM_AUTH'
	];

	let detail = $state<CognitoAppClientDetail | null>(null);
	let loading = $state(true);
	let secretVisible = $state(false);
	let saving = $state(false);

	let callbackText = $state('');
	let logoutText = $state('');
	let oauthFlows = $state<string[]>([]);
	let oauthScopes = $state<string[]>([]);
	let oauthEnabled = $state(false);
	let authFlows = $state<string[]>([]);
	// When off, the client uses the AWS default attribute access (all
	// readable, all mutable attrs writable) and the patch sends empty
	// lists to clear any prior custom set.
	let customPerms = $state(false);
	let readAttrs = $state<string[]>([]);
	let writeAttrs = $state<string[]>([]);

	onMount(load);

	async function load() {
		loading = true;
		try {
			const d = await describeAppClient(poolId, clientId);
			detail = d;
			callbackText = d.callbackURLs.join('\n');
			logoutText = d.logoutURLs.join('\n');
			oauthFlows = [...d.allowedOAuthFlows];
			oauthScopes = [...d.allowedOAuthScopes];
			oauthEnabled = d.allowedOAuthFlowsUserPoolClient ?? false;
			authFlows = [...d.explicitAuthFlows];
			// A non-empty list means the client has a custom set. Seed
			// each column independently from defaults when it is empty
			// (empty = "AWS default for that direction") so the matrix
			// always reflects effective access rather than a blank column.
			const defaults = defaultClientPerms(schema);
			customPerms = d.readAttributes.length > 0 || d.writeAttributes.length > 0;
			readAttrs = d.readAttributes.length > 0 ? [...d.readAttributes] : defaults.read;
			writeAttrs = d.writeAttributes.length > 0 ? [...d.writeAttributes] : defaults.write;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load client');
		} finally {
			loading = false;
		}
	}

	function toggleIn(arr: string[], value: string): string[] {
		return arr.includes(value) ? arr.filter((x) => x !== value) : [...arr, value];
	}

	function splitLines(s: string): string[] {
		return s
			.split(/\r?\n/)
			.map((x) => x.trim())
			.filter((x) => x.length > 0);
	}

	async function save() {
		saving = true;
		try {
			await updateAppClient({
				poolId,
				clientId,
				patch: {
					callbackURLs: splitLines(callbackText),
					logoutURLs: splitLines(logoutText),
					allowedOAuthFlows: oauthFlows,
					allowedOAuthScopes: oauthScopes,
					allowedOAuthFlowsUserPoolClient: oauthEnabled,
					explicitAuthFlows: authFlows,
					readAttributes: customPerms ? readAttrs : [],
					writeAttributes: customPerms ? writeAttrs : []
				}
			});
			toast.success('Client updated');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		} finally {
			saving = false;
		}
	}

	async function copyText(value: string) {
		try {
			await navigator.clipboard.writeText(value);
			toast.success('Copied');
		} catch {
			toast.error('Copy failed');
		}
	}
</script>

<div class="space-y-4 rounded border border-border/60 bg-muted/20 px-3 py-3">
	{#if loading}
		<p class="text-xs text-muted-foreground">
			<Loader2 class="inline size-3 animate-spin" /> Loading...
		</p>
	{:else if detail}
		<div class="grid gap-x-4 gap-y-1 text-sm sm:grid-cols-[120px_minmax(0,1fr)]">
			<div class="text-xs text-muted-foreground">Client ID</div>
			<div class="flex min-w-0 items-center gap-1">
				<code class="min-w-0 flex-1 truncate font-mono text-xs">{detail.clientId}</code>
				<Button
					variant="ghost"
					size="icon-sm"
					class="shrink-0"
					onclick={() => copyText(detail!.clientId)}
					title="Copy"
				>
					<Copy class="size-3" />
				</Button>
			</div>
			{#if detail.clientSecret}
				<div class="text-xs text-muted-foreground">Client secret</div>
				<div class="flex min-w-0 items-center gap-1">
					<code class="min-w-0 flex-1 truncate font-mono text-xs">
						{secretVisible ? detail.clientSecret : '•'.repeat(detail.clientSecret.length)}
					</code>
					<Button
						variant="ghost"
						size="icon-sm"
						class="shrink-0"
						onclick={() => (secretVisible = !secretVisible)}
						title={secretVisible ? 'Hide' : 'Reveal'}
					>
						{#if secretVisible}<EyeOff class="size-3" />{:else}<Eye class="size-3" />{/if}
					</Button>
					<Button
						variant="ghost"
						size="icon-sm"
						class="shrink-0"
						onclick={() => copyText(detail!.clientSecret ?? '')}
						title="Copy"
					>
						<Copy class="size-3" />
					</Button>
				</div>
			{/if}
		</div>

		<div class="space-y-1.5">
			<Label class="text-xs">Callback URLs (one per line)</Label>
			<textarea
				bind:value={callbackText}
				placeholder={`http://localhost:3000/callback`}
				class="block min-h-[60px] w-full resize-y rounded border border-border bg-background px-2 py-1.5 font-mono text-xs"
			></textarea>
		</div>

		<div class="space-y-1.5">
			<Label class="text-xs">Logout URLs (one per line)</Label>
			<textarea
				bind:value={logoutText}
				placeholder={`http://localhost:3000`}
				class="block min-h-[60px] w-full resize-y rounded border border-border bg-background px-2 py-1.5 font-mono text-xs"
			></textarea>
		</div>

		<div class="space-y-1.5">
			<label class="flex items-center gap-2 text-xs">
				<input type="checkbox" bind:checked={oauthEnabled} class="size-3.5" />
				Allow Cognito hosted UI / OAuth flows
			</label>
		</div>

		<div class="space-y-1.5">
			<Label class="text-xs">OAuth grants</Label>
			<div class="flex flex-wrap gap-1.5">
				{#each FLOWS as f (f)}
					<button
						type="button"
						class="rounded border px-2 py-0.5 text-xs transition-colors {oauthFlows.includes(f)
							? 'border-primary bg-primary/15 text-primary'
							: 'border-border bg-background text-muted-foreground'}"
						onclick={() => (oauthFlows = toggleIn(oauthFlows, f))}
					>
						{f}
					</button>
				{/each}
			</div>
		</div>

		<div class="space-y-1.5">
			<Label class="text-xs">OAuth scopes</Label>
			<div class="flex flex-wrap gap-1.5">
				{#each SCOPES as s (s)}
					<button
						type="button"
						class="rounded border px-2 py-0.5 font-mono text-xs transition-colors {oauthScopes.includes(s)
							? 'border-primary bg-primary/15 text-primary'
							: 'border-border bg-background text-muted-foreground'}"
						onclick={() => (oauthScopes = toggleIn(oauthScopes, s))}
					>
						{s}
					</button>
				{/each}
			</div>
		</div>

		<div class="space-y-1.5">
			<Label class="text-xs">Explicit auth flows (SDK auth)</Label>
			<div class="flex flex-wrap gap-1.5">
				{#each AUTH_FLOWS as f (f)}
					<button
						type="button"
						class="rounded border px-2 py-0.5 font-mono text-[10px] transition-colors {authFlows.includes(f)
							? 'border-primary bg-primary/15 text-primary'
							: 'border-border bg-background text-muted-foreground'}"
						onclick={() => (authFlows = toggleIn(authFlows, f))}
					>
						{f}
					</button>
				{/each}
			</div>
		</div>

		<div class="space-y-1.5">
			<label class="flex items-center gap-2 text-xs">
				<input type="checkbox" bind:checked={customPerms} class="size-3.5" />
				Set custom attribute read/write permissions
			</label>
			{#if customPerms}
				<p class="text-xs text-muted-foreground">
					Controls which user attributes this client can read and write via an
					access token. Immutable attributes cannot be granted write.
				</p>
				<AttributePermissions {schema} bind:read={readAttrs} bind:write={writeAttrs} />
			{:else}
				<p class="text-xs text-muted-foreground">
					Using the AWS default: all attributes readable, all mutable attributes
					writable. Enable to restrict per attribute.
				</p>
			{/if}
		</div>

		{#if detail.refreshTokenValidity || detail.accessTokenValidity || detail.idTokenValidity}
			<div class="flex flex-wrap gap-2 text-xs text-muted-foreground">
				{#if detail.refreshTokenValidity}
					<Badge variant="outline">refresh: {detail.refreshTokenValidity}</Badge>
				{/if}
				{#if detail.accessTokenValidity}
					<Badge variant="outline">access: {detail.accessTokenValidity}</Badge>
				{/if}
				{#if detail.idTokenValidity}
					<Badge variant="outline">id: {detail.idTokenValidity}</Badge>
				{/if}
			</div>
		{/if}

		<div class="flex justify-end">
			<Button size="sm" onclick={save} disabled={saving}>
				{#if saving}
					<Loader2 class="size-3.5 animate-spin" />
				{/if}
				Save
			</Button>
		</div>
	{/if}
</div>
