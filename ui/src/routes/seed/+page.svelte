<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		listUserPools,
		type UserPool
	} from '$lib/api/cognito';
	import {
		seedCognitoUsers,
		seedDynamoDb,
		seedS3,
		seedSecrets,
		seedSqs,
		type SeedCognitoResult,
		type SeedDdbResult,
		type SeedS3Result,
		type SeedSecretsResult,
		type SeedSqsResult
	} from '$lib/api';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Users from '@lucide/svelte/icons/users';
	import Database from '@lucide/svelte/icons/database';
	import HardDrive from '@lucide/svelte/icons/hard-drive';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import Inbox from '@lucide/svelte/icons/inbox';

	type Status = 'idle' | 'running' | 'done' | 'error';

	function num(s: string, dflt = 0): number {
		const n = Number(s);
		return Number.isFinite(n) && n >= 0 ? n : dflt;
	}

	function fmtMs(ms: number | undefined): string {
		if (ms === undefined) return '';
		if (ms < 1000) return `${ms} ms`;
		return `${(ms / 1000).toFixed(2)} s`;
	}

	function fmtBytes(bytes: number | undefined): string {
		if (bytes === undefined) return '';
		const units = ['B', 'KiB', 'MiB', 'GiB'];
		let v = bytes;
		let u = 0;
		while (v >= 1024 && u < units.length - 1) {
			v /= 1024;
			u++;
		}
		return `${v < 10 && u > 0 ? v.toFixed(1) : Math.round(v)} ${units[u]}`;
	}

	// ---- Cognito ----
	let pools = $state<UserPool[]>([]);
	let cogPoolId = $state('');
	let cogCount = $state('1000');
	let cogStatus = $state<Status>('idle');
	let cogResult = $state<SeedCognitoResult | null>(null);
	let cogError = $state<string | null>(null);

	onMount(async () => {
		try {
			const r = await listUserPools({ maxResults: 60 });
			pools = r.pools;
			if (pools.length > 0 && !cogPoolId) cogPoolId = pools[0].id;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load pools');
		}
	});

	const cogPoolLabel = $derived.by(() => {
		const p = pools.find((x) => x.id === cogPoolId);
		return p ? `${p.name} — ${p.id}` : '';
	});

	async function runCognito() {
		if (!cogPoolId) {
			toast.error('Pick a user pool first');
			return;
		}
		cogStatus = 'running';
		cogResult = null;
		cogError = null;
		try {
			cogResult = await seedCognitoUsers({
				pool_id: cogPoolId,
				count: num(cogCount, 1000)
			});
			cogStatus = 'done';
		} catch (e) {
			cogError = e instanceof Error ? e.message : 'Failed';
			cogStatus = 'error';
		}
	}

	// ---- DynamoDB ----
	let ddbTables = $state('5');
	let ddbItemsPerTable = $state('1000');
	let ddbStatus = $state<Status>('idle');
	let ddbResult = $state<SeedDdbResult | null>(null);
	let ddbError = $state<string | null>(null);

	async function runDdb() {
		ddbStatus = 'running';
		ddbResult = null;
		ddbError = null;
		try {
			ddbResult = await seedDynamoDb({
				tables: num(ddbTables, 5),
				items_per_table: num(ddbItemsPerTable, 1000)
			});
			ddbStatus = 'done';
		} catch (e) {
			ddbError = e instanceof Error ? e.message : 'Failed';
			ddbStatus = 'error';
		}
	}

	// ---- S3 ----
	let s3Buckets = $state('5');
	let s3ObjectsPerBucket = $state('100');
	let s3BodyBytes = $state('256');
	let s3Status = $state<Status>('idle');
	let s3Result = $state<SeedS3Result | null>(null);
	let s3Error = $state<string | null>(null);

	async function runS3() {
		s3Status = 'running';
		s3Result = null;
		s3Error = null;
		try {
			s3Result = await seedS3({
				buckets: num(s3Buckets, 5),
				objects_per_bucket: num(s3ObjectsPerBucket, 100),
				body_bytes: num(s3BodyBytes, 256)
			});
			s3Status = 'done';
		} catch (e) {
			s3Error = e instanceof Error ? e.message : 'Failed';
			s3Status = 'error';
		}
	}

	// ---- Secrets ----
	let secretsCount = $state('20');
	let secretsStatus = $state<Status>('idle');
	let secretsResult = $state<SeedSecretsResult | null>(null);
	let secretsError = $state<string | null>(null);

	async function runSecrets() {
		secretsStatus = 'running';
		secretsResult = null;
		secretsError = null;
		try {
			secretsResult = await seedSecrets({ count: num(secretsCount, 20) });
			secretsStatus = 'done';
		} catch (e) {
			secretsError = e instanceof Error ? e.message : 'Failed';
			secretsStatus = 'error';
		}
	}

	// ---- SQS ----
	let sqsQueues = $state('5');
	let sqsMessagesPerQueue = $state('50');
	let sqsStatus = $state<Status>('idle');
	let sqsResult = $state<SeedSqsResult | null>(null);
	let sqsError = $state<string | null>(null);

	async function runSqs() {
		sqsStatus = 'running';
		sqsResult = null;
		sqsError = null;
		try {
			sqsResult = await seedSqs({
				queues: num(sqsQueues, 5),
				messages_per_queue: num(sqsMessagesPerQueue, 50)
			});
			sqsStatus = 'done';
		} catch (e) {
			sqsError = e instanceof Error ? e.message : 'Failed';
			sqsStatus = 'error';
		}
	}

	function statusBadge(s: Status): { label: string; cls: string } {
		switch (s) {
			case 'running':
				return { label: 'running', cls: 'bg-orange-500/15 text-orange-300' };
			case 'done':
				return { label: 'done', cls: 'bg-emerald-500/15 text-emerald-300' };
			case 'error':
				return { label: 'error', cls: 'bg-destructive/15 text-destructive' };
			default:
				return { label: 'idle', cls: 'bg-muted text-muted-foreground' };
		}
	}

	const cogBadge = $derived(statusBadge(cogStatus));
	const ddbBadge = $derived(statusBadge(ddbStatus));
	const s3Badge = $derived(statusBadge(s3Status));
	const secretsBadge = $derived(statusBadge(secretsStatus));
	const sqsBadge = $derived(statusBadge(sqsStatus));
</script>

{#snippet stat(label: string, value: string | number)}
	<div class="flex flex-col rounded border border-border/60 bg-background/40 px-2 py-1.5">
		<span class="text-[10px] uppercase tracking-wide text-muted-foreground">{label}</span>
		<span class="font-mono text-xs text-foreground">{value}</span>
	</div>
{/snippet}

{#snippet errorBox(msg: string)}
	<p class="rounded border border-destructive/40 bg-destructive/5 px-2 py-1.5 font-mono text-[11px] text-destructive">
		{msg}
	</p>
{/snippet}

{#snippet sampleList(title: string, items: string[])}
	{#if items.length > 0}
		<div class="space-y-1">
			<div class="text-[10px] uppercase tracking-wide text-muted-foreground">{title}</div>
			<ul class="space-y-0.5 rounded border border-border/60 bg-background/40 p-1.5 font-mono text-[11px]">
				{#each items as item (item)}
					<li class="truncate text-muted-foreground" title={item}>{item}</li>
				{/each}
			</ul>
		</div>
	{/if}
{/snippet}

<ServicePage
	title="Seed data"
	description="Bulk-fill services with realistic fake data via /_awsim/seed/<service>. Skips SigV4 / gateway, so 10k-row seeds land in well under a second."
>
	<div class="grid h-full min-h-0 grid-cols-1 gap-4 overflow-y-auto p-6 sm:grid-cols-2 xl:grid-cols-3">
		<!-- Cognito -->
		<section class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
			<header class="flex items-center gap-2">
				<Users class="size-4 text-primary" />
				<h2 class="text-sm font-semibold">Cognito users</h2>
				<div class="flex-1"></div>
				<span class="rounded px-2 py-0.5 text-[10px] font-medium {cogBadge.cls}">
					{cogBadge.label}
				</span>
			</header>
			<p class="text-xs text-muted-foreground">
				Adds N users into a pool with random name + email + status mix.
			</p>
			<div class="space-y-2">
				<div class="space-y-1.5">
					<Label for="cog-pool">Pool</Label>
					<Select type="single" bind:value={cogPoolId} disabled={pools.length === 0}>
						<SelectTrigger id="cog-pool" class="w-full">
							{cogPoolId ? cogPoolLabel : '(no pools — create one first)'}
						</SelectTrigger>
						<SelectContent>
							{#each pools as p (p.id)}
								<SelectItem value={p.id} label={`${p.name} — ${p.id}`}>
									{p.name} — {p.id}
								</SelectItem>
							{/each}
						</SelectContent>
					</Select>
				</div>
				<div class="space-y-1.5">
					<Label for="cog-count">Count</Label>
					<Input id="cog-count" type="number" min="1" bind:value={cogCount} />
				</div>
			</div>
			{#if cogError}
				{@render errorBox(cogError)}
			{/if}
			{#if cogResult}
				<div class="space-y-2">
					<div class="grid grid-cols-3 gap-1.5">
						{@render stat('Created', cogResult.created.toLocaleString())}
						{@render stat('Skipped', cogResult.skipped.toLocaleString())}
						{@render stat('Time', fmtMs(cogResult.elapsed_ms))}
					</div>
					{#if cogResult.password}
						<div class="rounded border border-orange-500/30 bg-orange-500/5 p-2">
							<div class="text-[10px] uppercase tracking-wide text-orange-300/80">
								Password (every seeded user)
							</div>
							<code class="font-mono text-xs text-orange-200">{cogResult.password}</code>
						</div>
					{/if}
					{#if cogResult.status_breakdown}
						<div class="grid grid-cols-3 gap-1.5">
							{#each Object.entries(cogResult.status_breakdown) as [k, v] (k)}
								{@render stat(k, v.toLocaleString())}
							{/each}
						</div>
					{/if}
					{#if cogResult.sample_users && cogResult.sample_users.length > 0}
						<div class="space-y-1">
							<div class="text-[10px] uppercase tracking-wide text-muted-foreground">
								Sample users
							</div>
							<ul class="space-y-0.5 rounded border border-border/60 bg-background/40 p-1.5 font-mono text-[11px]">
								{#each cogResult.sample_users as u (u.username)}
									<li class="flex items-baseline justify-between gap-2 truncate">
										<span class="truncate text-foreground" title={u.username}>{u.username}</span>
										<span class="shrink-0 text-muted-foreground">{u.status}</span>
									</li>
								{/each}
							</ul>
						</div>
					{/if}
				</div>
			{/if}
			<Button
				size="sm"
				onclick={runCognito}
				disabled={cogStatus === 'running' || !cogPoolId}
				class="self-start"
			>
				{#if cogStatus === 'running'}<Loader2 class="size-3.5 animate-spin" />{/if}
				Run
			</Button>
		</section>

		<!-- DynamoDB -->
		<section class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
			<header class="flex items-center gap-2">
				<Database class="size-4 text-primary" />
				<h2 class="text-sm font-semibold">DynamoDB</h2>
				<div class="flex-1"></div>
				<span class="rounded px-2 py-0.5 text-[10px] font-medium {ddbBadge.cls}">
					{ddbBadge.label}
				</span>
			</header>
			<p class="text-xs text-muted-foreground">
				N tables, each with an <code>id</code> hash key and M random items.
			</p>
			<div class="grid grid-cols-2 gap-2">
				<div class="space-y-1.5">
					<Label for="ddb-tables">Tables</Label>
					<Input id="ddb-tables" type="number" min="1" bind:value={ddbTables} />
				</div>
				<div class="space-y-1.5">
					<Label for="ddb-items">Items / table</Label>
					<Input
						id="ddb-items"
						type="number"
						min="0"
						bind:value={ddbItemsPerTable}
					/>
				</div>
			</div>
			{#if ddbError}
				{@render errorBox(ddbError)}
			{/if}
			{#if ddbResult}
				<div class="space-y-2">
					<div class="grid grid-cols-3 gap-1.5">
						{@render stat('Tables', ddbResult.tables_created.toLocaleString())}
						{@render stat('Items', ddbResult.items_created.toLocaleString())}
						{@render stat('Time', fmtMs(ddbResult.elapsed_ms))}
					</div>
					{#if ddbResult.errors.length > 0}
						<div class="rounded border border-destructive/40 bg-destructive/5 p-1.5 font-mono text-[11px] text-destructive">
							{ddbResult.errors.length} error(s) — first: {ddbResult.errors[0]}
						</div>
					{/if}
					{@render sampleList('Sample tables', ddbResult.sample_tables ?? [])}
				</div>
			{/if}
			<Button
				size="sm"
				onclick={runDdb}
				disabled={ddbStatus === 'running'}
				class="self-start"
			>
				{#if ddbStatus === 'running'}<Loader2 class="size-3.5 animate-spin" />{/if}
				Run
			</Button>
		</section>

		<!-- S3 -->
		<section class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
			<header class="flex items-center gap-2">
				<HardDrive class="size-4 text-primary" />
				<h2 class="text-sm font-semibold">S3</h2>
				<div class="flex-1"></div>
				<span class="rounded px-2 py-0.5 text-[10px] font-medium {s3Badge.cls}">
					{s3Badge.label}
				</span>
			</header>
			<p class="text-xs text-muted-foreground">
				N buckets with M small text objects each (capped at 64 KiB body).
			</p>
			<div class="grid grid-cols-3 gap-2">
				<div class="space-y-1.5">
					<Label for="s3-buckets">Buckets</Label>
					<Input id="s3-buckets" type="number" min="1" bind:value={s3Buckets} />
				</div>
				<div class="space-y-1.5">
					<Label for="s3-objects">Objects / bucket</Label>
					<Input id="s3-objects" type="number" min="0" bind:value={s3ObjectsPerBucket} />
				</div>
				<div class="space-y-1.5">
					<Label for="s3-bytes">Body bytes</Label>
					<Input id="s3-bytes" type="number" min="0" max="65536" bind:value={s3BodyBytes} />
				</div>
			</div>
			{#if s3Error}
				{@render errorBox(s3Error)}
			{/if}
			{#if s3Result}
				<div class="space-y-2">
					<div class="grid grid-cols-2 gap-1.5">
						{@render stat('Buckets', s3Result.buckets_created.toLocaleString())}
						{@render stat('Objects', s3Result.objects_created.toLocaleString())}
						{@render stat('Bytes', fmtBytes(s3Result.bytes_written))}
						{@render stat('Time', fmtMs(s3Result.elapsed_ms))}
					</div>
					{@render sampleList('Sample buckets', s3Result.sample_buckets ?? [])}
				</div>
			{/if}
			<Button size="sm" onclick={runS3} disabled={s3Status === 'running'} class="self-start">
				{#if s3Status === 'running'}<Loader2 class="size-3.5 animate-spin" />{/if}
				Run
			</Button>
		</section>

		<!-- Secrets -->
		<section class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
			<header class="flex items-center gap-2">
				<KeyRound class="size-4 text-primary" />
				<h2 class="text-sm font-semibold">Secrets Manager</h2>
				<div class="flex-1"></div>
				<span class="rounded px-2 py-0.5 text-[10px] font-medium {secretsBadge.cls}">
					{secretsBadge.label}
				</span>
			</header>
			<p class="text-xs text-muted-foreground">
				N secrets with credential-shaped JSON bodies (username / password / host / port).
			</p>
			<div class="space-y-1.5">
				<Label for="sec-count">Count</Label>
				<Input id="sec-count" type="number" min="1" bind:value={secretsCount} />
			</div>
			{#if secretsError}
				{@render errorBox(secretsError)}
			{/if}
			{#if secretsResult}
				<div class="space-y-2">
					<div class="grid grid-cols-2 gap-1.5">
						{@render stat('Created', secretsResult.created.toLocaleString())}
						{@render stat('Time', fmtMs(secretsResult.elapsed_ms))}
					</div>
					{@render sampleList(
						'Sample secrets',
						(secretsResult.sample_secrets ?? []).map((s) => s.name)
					)}
				</div>
			{/if}
			<Button
				size="sm"
				onclick={runSecrets}
				disabled={secretsStatus === 'running'}
				class="self-start"
			>
				{#if secretsStatus === 'running'}<Loader2 class="size-3.5 animate-spin" />{/if}
				Run
			</Button>
		</section>

		<!-- SQS -->
		<section class="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
			<header class="flex items-center gap-2">
				<Inbox class="size-4 text-primary" />
				<h2 class="text-sm font-semibold">SQS</h2>
				<div class="flex-1"></div>
				<span class="rounded px-2 py-0.5 text-[10px] font-medium {sqsBadge.cls}">
					{sqsBadge.label}
				</span>
			</header>
			<p class="text-xs text-muted-foreground">
				N standard queues with M random-sentence messages each (md5'd correctly).
			</p>
			<div class="grid grid-cols-2 gap-2">
				<div class="space-y-1.5">
					<Label for="sqs-queues">Queues</Label>
					<Input id="sqs-queues" type="number" min="1" bind:value={sqsQueues} />
				</div>
				<div class="space-y-1.5">
					<Label for="sqs-msgs">Messages / queue</Label>
					<Input id="sqs-msgs" type="number" min="0" bind:value={sqsMessagesPerQueue} />
				</div>
			</div>
			{#if sqsError}
				{@render errorBox(sqsError)}
			{/if}
			{#if sqsResult}
				<div class="space-y-2">
					<div class="grid grid-cols-3 gap-1.5">
						{@render stat('Queues', sqsResult.queues_created.toLocaleString())}
						{@render stat('Messages', sqsResult.messages_created.toLocaleString())}
						{@render stat('Time', fmtMs(sqsResult.elapsed_ms))}
					</div>
					{@render sampleList(
						'Sample queues',
						(sqsResult.sample_queues ?? []).map((q) => q.name)
					)}
				</div>
			{/if}
			<Button size="sm" onclick={runSqs} disabled={sqsStatus === 'running'} class="self-start">
				{#if sqsStatus === 'running'}<Loader2 class="size-3.5 animate-spin" />{/if}
				Run
			</Button>
		</section>
	</div>
</ServicePage>
