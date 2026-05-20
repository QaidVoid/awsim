<script lang="ts">
	import { onMount } from 'svelte';
	import {
		getRuntimeConfig,
		getRuntimeConfigDefaults,
		putRuntimeConfig,
		type RuntimeConfig,
		type RuntimeConfigEnvelope,
	} from '$lib/api/runtime-config';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Switch } from '$lib/components/ui/switch';
	import { Badge } from '$lib/components/ui/badge';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import SaveIcon from '@lucide/svelte/icons/save';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import NetworkIcon from '@lucide/svelte/icons/network';
	import { route } from '$lib/url';
	import { toast } from 'svelte-sonner';

	let envelope = $state<RuntimeConfigEnvelope | null>(null);
	let configDefaults = $state<RuntimeConfig | null>(null);
	let loading = $state(true);
	let saving = $state(false);

	let sesRetentionHours = $state(720);
	let iamEnforce = $state(false);
	let logLevel = $state('info');

	onMount(load);

	async function load() {
		loading = true;
		try {
			const [env, cfgDefs] = await Promise.all([
				getRuntimeConfig(),
				getRuntimeConfigDefaults(),
			]);
			envelope = env;
			configDefaults = cfgDefs;
			seedFromEnvelope(env.config);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load runtime config');
		} finally {
			loading = false;
		}
	}

	function resetSes() {
		if (!configDefaults) return;
		sesRetentionHours = configDefaults.ses.retention_hours;
		toast.info('SES section reset — Save to apply');
	}
	function resetIam() {
		if (!configDefaults) return;
		iamEnforce = configDefaults.iam.enforce;
		toast.info('IAM section reset — Save to apply');
	}
	function resetLogging() {
		if (!configDefaults) return;
		logLevel = configDefaults.logging.level;
		toast.info('Logging section reset — Save to apply');
	}

	let isSesModified = $derived(
		!!configDefaults && sesRetentionHours !== configDefaults.ses.retention_hours
	);
	let isIamModified = $derived(!!configDefaults && iamEnforce !== configDefaults.iam.enforce);
	let isLoggingModified = $derived(
		!!configDefaults && (logLevel.trim() || 'info') !== configDefaults.logging.level
	);

	function seedFromEnvelope(cfg: RuntimeConfig) {
		sesRetentionHours = cfg.ses.retention_hours;
		iamEnforce = cfg.iam.enforce;
		logLevel = cfg.logging.level;
	}

	// Save by re-fetching the latest config first, then mutating
	// only the three sections this page owns. Avoids stomping on
	// Bedrock changes the Model Gateway page may have written
	// since we last loaded.
	async function save() {
		saving = true;
		try {
			const latest = await getRuntimeConfig();
			const payload: RuntimeConfig = {
				...latest.config,
				ses: { retention_hours: sesRetentionHours },
				iam: { enforce: iamEnforce },
				logging: { level: logLevel.trim() || 'info' },
			};
			envelope = await putRuntimeConfig(payload);
			seedFromEnvelope(envelope.config);
			toast.success('Settings saved');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Save failed');
		} finally {
			saving = false;
		}
	}
</script>

{#snippet headerActions()}
	<Button variant="ghost" size="sm" onclick={load} disabled={loading || saving}>
		<RefreshCwIcon class={loading ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />
		<span class="ml-2">Reload</span>
	</Button>
	<Button size="sm" onclick={save} disabled={loading || saving}>
		<SaveIcon class="h-4 w-4" />
		<span class="ml-2">{saving ? 'Saving…' : 'Save'}</span>
	</Button>
{/snippet}

<ServicePage
	title="Settings"
	description="Hot-reloadable runtime configuration. Changes apply immediately."
	actions={headerActions}
>
	<div class="space-y-6 p-6">
		{#if envelope && !envelope.persistent}
			<Alert>
				<CircleAlertIcon class="h-4 w-4" />
				<AlertTitle>In-memory only</AlertTitle>
				<AlertDescription>
					Changes apply for the current run but reset on restart. Pass <code>--data-dir</code> to persist settings.
				</AlertDescription>
			</Alert>
		{:else if envelope?.persistent && envelope.configPath}
			<Alert>
				<HardDriveIcon class="h-4 w-4" />
				<AlertTitle>Persisted</AlertTitle>
				<AlertDescription>
					Settings save to <code class="font-mono text-xs">{envelope.configPath}</code> and survive restarts.
				</AlertDescription>
			</Alert>
		{/if}

		<!-- Bedrock proxy: deferral card. The full editor lives on
		     the Model Gateway page now (Phase 8 of the gateway
		     overhaul moved it). Existing TOML configs keep loading
		     unchanged; this card is just a signpost. -->
		<section class="rounded-lg border bg-card">
			<div class="flex items-start gap-3 p-4">
				<NetworkIcon class="mt-0.5 h-5 w-5 shrink-0 text-muted-foreground" />
				<div class="flex-1 space-y-1">
					<h2 class="text-base font-semibold">Bedrock proxy</h2>
					<p class="text-sm text-muted-foreground">
						Backends, credentials, model aliases, routing, and health all live on the
						<a class="underline" href={route('/gateway')}>Model Gateway</a> page now. Legacy
						<code class="font-mono text-xs">[invoke]</code> / <code class="font-mono text-xs">[embed]</code>
						entries in any existing <code class="font-mono text-xs">runtime-config.json</code> keep
						loading unchanged.
					</p>
				</div>
				<Button variant="outline" size="sm" href={route('/gateway') + '?tab=backends'}>
					Open Model Gateway
				</Button>
			</div>
		</section>

		<!-- SES section -->
		<section class="rounded-lg border bg-card">
			<header class="flex items-start justify-between gap-4 border-b p-4">
				<div>
					<div class="flex items-center gap-2">
						<h2 class="text-base font-semibold">SES outbox retention</h2>
						{#if isSesModified}
							<Badge variant="secondary" class="text-[10px]">modified</Badge>
						{/if}
					</div>
					<p class="mt-1 text-sm text-muted-foreground">
						Hours to retain captured outbound emails before the hourly sweep deletes them.
						Set to 0 to keep all emails forever.
					</p>
				</div>
				{#if isSesModified}
					<Button variant="ghost" size="sm" onclick={resetSes}>
						<RotateCcwIcon class="h-4 w-4" />
						<span class="ml-1">Reset</span>
					</Button>
				{/if}
			</header>
			<div class="p-4">
				<Label for="ses-retention">Retention hours</Label>
				<Input
					id="ses-retention"
					type="number"
					min="0"
					bind:value={sesRetentionHours}
					class="max-w-xs"
				/>
				<p class="mt-1 text-xs text-muted-foreground">
					Default: 720 (30 days). Sweep runs once per hour.
				</p>
			</div>
		</section>

		<!-- IAM section -->
		<section class="rounded-lg border bg-card">
			<header class="flex items-start justify-between gap-4 border-b p-4">
				<div>
					<div class="flex items-center gap-2">
						<h2 class="text-base font-semibold">IAM enforcement</h2>
						{#if isIamModified}
							<Badge variant="secondary" class="text-[10px]">modified</Badge>
						{/if}
					</div>
					<p class="mt-1 text-sm text-muted-foreground">
						When on, every request runs through the IAM policy engine: identity policies,
						resource policies, SCPs, KMS grants. When off, all calls are allowed regardless
						of identity. Off is the default for ergonomic local dev; flip on to test
						policy logic.
					</p>
				</div>
				<div class="flex items-center gap-2 pt-1">
					{#if isIamModified}
						<Button variant="ghost" size="sm" onclick={resetIam}>
							<RotateCcwIcon class="h-4 w-4" />
							<span class="ml-1">Reset</span>
						</Button>
					{/if}
					<Label for="iam-enforce" class="text-sm">Enforce</Label>
					<Switch id="iam-enforce" bind:checked={iamEnforce} />
				</div>
			</header>
			<div class="p-4 text-xs text-muted-foreground">
				Equivalent CLI flag:
				<code class="ml-1 rounded bg-muted px-1.5 py-0.5 font-mono">AWSIM_IAM_ENFORCE=true</code>
			</div>
		</section>

		<!-- Logging section -->
		<section class="rounded-lg border bg-card">
			<header class="flex items-start justify-between gap-4 border-b p-4">
				<div>
					<div class="flex items-center gap-2">
						<h2 class="text-base font-semibold">Log level</h2>
						{#if isLoggingModified}
							<Badge variant="secondary" class="text-[10px]">modified</Badge>
						{/if}
					</div>
					<p class="mt-1 text-sm text-muted-foreground">
						Tracing filter directive. Same syntax as the <code>RUST_LOG</code> env var:
						<code>info</code>, <code>debug</code>, or per-target overrides like
						<code>info,awsim_dynamodb=debug,sqlx=warn</code>. Hot-reloaded — flip to
						<code>debug</code> to capture more detail without restarting.
					</p>
				</div>
				{#if isLoggingModified}
					<Button variant="ghost" size="sm" onclick={resetLogging}>
						<RotateCcwIcon class="h-4 w-4" />
						<span class="ml-1">Reset</span>
					</Button>
				{/if}
			</header>
			<div class="space-y-2 p-4">
				<div class="flex flex-wrap items-center gap-2">
					<Label for="log-level" class="text-sm shrink-0">Filter</Label>
					<Input
						id="log-level"
						bind:value={logLevel}
						placeholder="info"
						class="max-w-md"
					/>
					<div class="flex flex-wrap gap-1">
						{#each ['error', 'warn', 'info', 'debug', 'trace'] as preset (preset)}
							<Button
								variant="outline"
								size="sm"
								onclick={() => (logLevel = preset)}
							>
								{preset}
							</Button>
						{/each}
					</div>
				</div>
			</div>
		</section>

		<!-- Restart-required values -->
		<section class="rounded-lg border bg-card">
			<header class="border-b p-4">
				<h2 class="text-base font-semibold">Restart required</h2>
				<p class="mt-1 text-sm text-muted-foreground">
					These settings are baked in at startup. Pass the matching CLI flag and restart awsim to change them.
				</p>
			</header>
			<div class="p-4 text-sm">
				<ul class="space-y-1 text-muted-foreground">
					<li>
						<Badge variant="outline" class="mr-2 font-mono text-[11px]">--port</Badge> Server listen port
					</li>
					<li>
						<Badge variant="outline" class="mr-2 font-mono text-[11px]">--region</Badge>
						<Badge variant="outline" class="mr-2 font-mono text-[11px]">--account-id</Badge>
						Default AWS coordinates
					</li>
					<li>
						<Badge variant="outline" class="mr-2 font-mono text-[11px]">--data-dir</Badge> Persistence directory
					</li>
					<li>
						<Badge variant="outline" class="mr-2 font-mono text-[11px]">--max-concurrent-requests</Badge>
						Inflight cap (load shedding)
					</li>
					<li>
						<Badge variant="outline" class="mr-2 font-mono text-[11px]">--max-body-bytes</Badge> Request body cap
					</li>
				</ul>
			</div>
		</section>

		<!-- Footer with persistence info -->
		{#if envelope}
			<footer class="flex items-center gap-2 border-t pt-4 text-xs text-muted-foreground">
				<HardDriveIcon class="h-3.5 w-3.5" />
				{#if envelope.persistent && envelope.configPath}
					<span>
						Persisted at
						<code class="rounded bg-muted px-1.5 py-0.5 font-mono">{envelope.configPath}</code>
					</span>
				{:else}
					<span>In-memory only — pass <code class="font-mono">--data-dir</code> to persist.</span>
				{/if}
			</footer>
		{/if}
	</div>
</ServicePage>
