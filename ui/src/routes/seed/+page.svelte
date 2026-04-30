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
		seedSqs
	} from '$lib/api';
	import { ServicePage } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
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

	// ---- Cognito ----
	let pools = $state<UserPool[]>([]);
	let cogPoolId = $state('');
	let cogCount = $state('1000');
	let cogStatus = $state<Status>('idle');
	let cogResult = $state<string | null>(null);

	onMount(async () => {
		try {
			const r = await listUserPools({ maxResults: 60 });
			pools = r.pools;
			if (pools.length > 0 && !cogPoolId) cogPoolId = pools[0].id;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load pools');
		}
	});

	async function runCognito() {
		if (!cogPoolId) {
			toast.error('Pick a user pool first');
			return;
		}
		cogStatus = 'running';
		cogResult = null;
		try {
			const r = await seedCognitoUsers({
				pool_id: cogPoolId,
				count: num(cogCount, 1000)
			});
			cogResult = `Created ${r.created.toLocaleString()} users (${r.skipped} skipped).`;
			cogStatus = 'done';
		} catch (e) {
			cogResult = e instanceof Error ? e.message : 'Failed';
			cogStatus = 'error';
		}
	}

	// ---- DynamoDB ----
	let ddbTables = $state('5');
	let ddbItemsPerTable = $state('1000');
	let ddbStatus = $state<Status>('idle');
	let ddbResult = $state<string | null>(null);

	async function runDdb() {
		ddbStatus = 'running';
		ddbResult = null;
		try {
			const r = await seedDynamoDb({
				tables: num(ddbTables, 5),
				items_per_table: num(ddbItemsPerTable, 1000)
			});
			ddbResult = `Created ${r.tables_created} tables, ${r.items_created.toLocaleString()} items${
				r.errors.length > 0 ? ` (${r.errors.length} errors)` : ''
			}.`;
			ddbStatus = 'done';
		} catch (e) {
			ddbResult = e instanceof Error ? e.message : 'Failed';
			ddbStatus = 'error';
		}
	}

	// ---- S3 ----
	let s3Buckets = $state('5');
	let s3ObjectsPerBucket = $state('100');
	let s3BodyBytes = $state('256');
	let s3Status = $state<Status>('idle');
	let s3Result = $state<string | null>(null);

	async function runS3() {
		s3Status = 'running';
		s3Result = null;
		try {
			const r = await seedS3({
				buckets: num(s3Buckets, 5),
				objects_per_bucket: num(s3ObjectsPerBucket, 100),
				body_bytes: num(s3BodyBytes, 256)
			});
			s3Result = `Created ${r.buckets_created} buckets, ${r.objects_created.toLocaleString()} objects.`;
			s3Status = 'done';
		} catch (e) {
			s3Result = e instanceof Error ? e.message : 'Failed';
			s3Status = 'error';
		}
	}

	// ---- Secrets ----
	let secretsCount = $state('20');
	let secretsStatus = $state<Status>('idle');
	let secretsResult = $state<string | null>(null);

	async function runSecrets() {
		secretsStatus = 'running';
		secretsResult = null;
		try {
			const r = await seedSecrets({ count: num(secretsCount, 20) });
			secretsResult = `Created ${r.created.toLocaleString()} secrets.`;
			secretsStatus = 'done';
		} catch (e) {
			secretsResult = e instanceof Error ? e.message : 'Failed';
			secretsStatus = 'error';
		}
	}

	// ---- SQS ----
	let sqsQueues = $state('5');
	let sqsMessagesPerQueue = $state('50');
	let sqsStatus = $state<Status>('idle');
	let sqsResult = $state<string | null>(null);

	async function runSqs() {
		sqsStatus = 'running';
		sqsResult = null;
		try {
			const r = await seedSqs({
				queues: num(sqsQueues, 5),
				messages_per_queue: num(sqsMessagesPerQueue, 50)
			});
			sqsResult = `Created ${r.queues_created} queues, ${r.messages_created.toLocaleString()} messages.`;
			sqsStatus = 'done';
		} catch (e) {
			sqsResult = e instanceof Error ? e.message : 'Failed';
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
					<select
						id="cog-pool"
						bind:value={cogPoolId}
						class="h-9 w-full rounded-md border border-border bg-background px-2 text-sm"
					>
						{#if pools.length === 0}
							<option value="" disabled>(no pools — create one first)</option>
						{:else}
							{#each pools as p (p.id)}
								<option value={p.id}>{p.name} — {p.id}</option>
							{/each}
						{/if}
					</select>
				</div>
				<div class="space-y-1.5">
					<Label for="cog-count">Count</Label>
					<Input id="cog-count" type="number" min="1" bind:value={cogCount} />
				</div>
			</div>
			{#if cogResult}
				<p
					class="rounded border px-2 py-1.5 font-mono text-[11px] {cogStatus === 'error'
						? 'border-destructive/40 text-destructive'
						: 'border-border/60 text-muted-foreground'}"
				>
					{cogResult}
				</p>
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
			{#if ddbResult}
				<p
					class="rounded border px-2 py-1.5 font-mono text-[11px] {ddbStatus === 'error'
						? 'border-destructive/40 text-destructive'
						: 'border-border/60 text-muted-foreground'}"
				>
					{ddbResult}
				</p>
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
			{#if s3Result}
				<p
					class="rounded border px-2 py-1.5 font-mono text-[11px] {s3Status === 'error'
						? 'border-destructive/40 text-destructive'
						: 'border-border/60 text-muted-foreground'}"
				>
					{s3Result}
				</p>
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
			{#if secretsResult}
				<p
					class="rounded border px-2 py-1.5 font-mono text-[11px] {secretsStatus === 'error'
						? 'border-destructive/40 text-destructive'
						: 'border-border/60 text-muted-foreground'}"
				>
					{secretsResult}
				</p>
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
			{#if sqsResult}
				<p
					class="rounded border px-2 py-1.5 font-mono text-[11px] {sqsStatus === 'error'
						? 'border-destructive/40 text-destructive'
						: 'border-border/60 text-muted-foreground'}"
				>
					{sqsResult}
				</p>
			{/if}
			<Button size="sm" onclick={runSqs} disabled={sqsStatus === 'running'} class="self-start">
				{#if sqsStatus === 'running'}<Loader2 class="size-3.5 animate-spin" />{/if}
				Run
			</Button>
		</section>
	</div>
</ServicePage>
