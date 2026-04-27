<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listUsers,
		listRoles,
		simulatePrincipalPolicy,
		simulateCustomPolicy,
		ACTION_SUGGESTIONS,
		type SimulationResult,
		type ContextEntry
	} from '$lib/api/iam';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
	import PolicyEditor from './policy-editor.svelte';
	import Play from '@lucide/svelte/icons/play';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import ShieldX from '@lucide/svelte/icons/shield-x';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import { toast } from 'svelte-sonner';

	// Principal simulator state
	let principals = $state<{ value: string; label: string; kind: 'user' | 'role' }[]>([]);
	let principalArn = $state('');
	let actionInput = $state('');
	let actions = $state<string[]>([]);
	let resourceArn = $state('');
	let contextEntries = $state<ContextEntry[]>([]);
	let principalLoading = $state(false);
	let principalResult = $state<SimulationResult | null>(null);

	// Custom simulator state
	let customPolicy = $state(
		JSON.stringify(
			{
				Version: '2012-10-17',
				Statement: [
					{
						Effect: 'Allow',
						Action: ['s3:GetObject'],
						Resource: ['arn:aws:s3:::example-bucket/*']
					}
				]
			},
			null,
			2
		)
	);
	let customAction = $state('s3:GetObject');
	let customResource = $state('arn:aws:s3:::example-bucket/key');
	let customLoading = $state(false);
	let customResult = $state<SimulationResult | null>(null);

	// Action autocomplete
	let showActionSuggestions = $state(false);
	const filteredActions = $derived(
		actionInput.trim()
			? ACTION_SUGGESTIONS.filter((a) =>
					a.toLowerCase().includes(actionInput.trim().toLowerCase())
				).slice(0, 8)
			: ACTION_SUGGESTIONS.slice(0, 8)
	);

	async function loadPrincipals() {
		try {
			const [users, roles] = await Promise.all([listUsers(), listRoles()]);
			principals = [
				...users.map((u) => ({
					value: u.arn,
					label: u.userName,
					kind: 'user' as const
				})),
				...roles.map((r) => ({
					value: r.arn,
					label: r.roleName,
					kind: 'role' as const
				}))
			];
			if (!principalArn && principals.length > 0) principalArn = principals[0].value;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load principals');
		}
	}

	function addAction() {
		const a = actionInput.trim();
		if (!a) return;
		if (!actions.includes(a)) actions = [...actions, a];
		actionInput = '';
		showActionSuggestions = false;
	}

	function pickAction(a: string) {
		if (!actions.includes(a)) actions = [...actions, a];
		actionInput = '';
		showActionSuggestions = false;
	}

	function removeAction(a: string) {
		actions = actions.filter((x) => x !== a);
	}

	function addContext() {
		contextEntries = [...contextEntries, { key: '', values: [''], type: 'string' }];
	}

	function removeContext(idx: number) {
		contextEntries = contextEntries.filter((_, i) => i !== idx);
	}

	async function runPrincipal() {
		if (!principalArn) {
			toast.error('Pick a principal');
			return;
		}
		if (actions.length === 0) {
			toast.error('Add at least one action');
			return;
		}
		principalLoading = true;
		principalResult = null;
		try {
			const cleaned = contextEntries
				.filter((e) => e.key.trim())
				.map((e) => ({
					key: e.key.trim(),
					values: e.values.filter((v) => v.trim()),
					type: e.type || 'string'
				}));
			principalResult = await simulatePrincipalPolicy({
				policySourceArn: principalArn,
				actions,
				resources: resourceArn.trim() ? [resourceArn.trim()] : undefined,
				contextEntries: cleaned.length ? cleaned : undefined
			});
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Simulation failed');
		} finally {
			principalLoading = false;
		}
	}

	async function runCustom() {
		if (!customAction.trim()) {
			toast.error('Set an action');
			return;
		}
		try {
			JSON.parse(customPolicy);
		} catch {
			toast.error('Policy is not valid JSON');
			return;
		}
		customLoading = true;
		customResult = null;
		try {
			customResult = await simulateCustomPolicy({
				policyInputList: [customPolicy],
				actions: [customAction.trim()],
				resources: customResource.trim() ? [customResource.trim()] : undefined
			});
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Simulation failed');
		} finally {
			customLoading = false;
		}
	}

	function decisionLabel(d: 'allowed' | 'explicitDeny' | 'implicitDeny'): string {
		if (d === 'allowed') return 'ALLOWED';
		if (d === 'explicitDeny') return 'EXPLICITLY DENIED';
		return 'IMPLICITLY DENIED';
	}

	onMount(loadPrincipals);
</script>

<div class="space-y-6 p-6">
	{#if principalResult}
		<div class="space-y-2">
			{#each principalResult.results as r (r.evalActionName)}
				{@const allowed = r.evalDecision === 'allowed'}
				<div
					class="flex items-center gap-4 rounded-lg border-2 p-4 {allowed
						? 'border-emerald-500/40 bg-emerald-500/5'
						: 'border-destructive/40 bg-destructive/5'}"
				>
					{#if allowed}
						<ShieldCheck class="size-10 text-emerald-500" />
					{:else}
						<ShieldX class="size-10 text-destructive" />
					{/if}
					<div class="min-w-0 flex-1">
						<div class="text-xs uppercase tracking-wider text-muted-foreground">
							{r.evalActionName}
						</div>
						<div class="text-2xl font-bold {allowed ? 'text-emerald-500' : 'text-destructive'}">
							{decisionLabel(r.evalDecision)}
						</div>
						{#if r.evalResourceName}
							<div class="truncate font-mono text-xs text-muted-foreground">
								on {r.evalResourceName}
							</div>
						{/if}
					</div>
					<div class="flex flex-col items-end gap-1 text-xs">
						{#if r.matchedStatements.length > 0}
							<Badge variant="outline">
								{r.matchedStatements.length} matched statement{r.matchedStatements.length === 1
									? ''
									: 's'}
							</Badge>
						{/if}
						{#if r.missingContextValues.length > 0}
							<Badge variant="destructive">
								{r.missingContextValues.length} missing context
							</Badge>
						{/if}
					</div>
				</div>
				{#if r.matchedStatements.length || r.missingContextValues.length}
					<div class="grid grid-cols-2 gap-2 text-xs">
						{#if r.matchedStatements.length}
							<div class="rounded border border-border/60 p-3">
								<div class="mb-1.5 font-semibold uppercase tracking-wide text-muted-foreground">
									Matched statements
								</div>
								<ul class="space-y-1">
									{#each r.matchedStatements as s, i (s.sourcePolicyId + i)}
										<li class="font-mono">
											<span class="text-muted-foreground">{s.sourcePolicyType}:</span>
											{s.sourcePolicyId}
										</li>
									{/each}
								</ul>
							</div>
						{/if}
						{#if r.missingContextValues.length}
							<div class="rounded border border-destructive/30 bg-destructive/5 p-3">
								<div class="mb-1.5 font-semibold uppercase tracking-wide text-muted-foreground">
									Missing context keys
								</div>
								<ul class="space-y-1">
									{#each r.missingContextValues as k, i (k + i)}
										<li class="font-mono">{k}</li>
									{/each}
								</ul>
							</div>
						{/if}
					</div>
				{/if}
			{/each}
		</div>
	{/if}

	<Card>
		<CardHeader>
			<CardTitle class="flex items-center gap-2">
				<Sparkles class="size-4 text-primary" /> Simulate principal policy
			</CardTitle>
		</CardHeader>
		<CardContent class="grid gap-4">
			<div class="grid gap-3 md:grid-cols-2">
				<div class="flex flex-col gap-1.5">
					<Label for="sim-principal" class="text-xs">Principal (user / role)</Label>
					<select
						id="sim-principal"
						bind:value={principalArn}
						class="h-9 rounded-md border border-input bg-background px-2.5 text-sm shadow-xs"
					>
						{#if principals.length === 0}
							<option value="">No users or roles found</option>
						{/if}
						{#each principals as p (p.value)}
							<option value={p.value}>[{p.kind}] {p.label}</option>
						{/each}
					</select>
				</div>
				<div class="flex flex-col gap-1.5">
					<Label for="sim-resource" class="text-xs">Resource ARN (optional)</Label>
					<Input
						id="sim-resource"
						bind:value={resourceArn}
						placeholder="arn:aws:s3:::my-bucket/*"
						class="font-mono text-xs"
					/>
				</div>
			</div>

			<div class="flex flex-col gap-1.5">
				<Label for="sim-action" class="text-xs">Actions</Label>
				<div class="relative">
					<Input
						id="sim-action"
						bind:value={actionInput}
						placeholder="s3:GetObject (Tab to add)"
						class="font-mono text-xs"
						onfocus={() => (showActionSuggestions = true)}
						onblur={() => setTimeout(() => (showActionSuggestions = false), 150)}
						onkeydown={(e) => {
							if (e.key === 'Enter' || e.key === 'Tab') {
								if (actionInput.trim()) {
									e.preventDefault();
									addAction();
								}
							}
						}}
					/>
					{#if showActionSuggestions && filteredActions.length > 0}
						<div
							class="absolute left-0 right-0 top-full z-10 mt-1 max-h-56 overflow-y-auto rounded-md border border-border bg-popover shadow-md"
						>
							{#each filteredActions as a (a)}
								<button
									type="button"
									class="block w-full px-3 py-1.5 text-left font-mono text-xs hover:bg-muted"
									onmousedown={(e) => {
										e.preventDefault();
										pickAction(a);
									}}
								>
									{a}
								</button>
							{/each}
						</div>
					{/if}
				</div>
				{#if actions.length > 0}
					<div class="flex flex-wrap gap-1.5 pt-1">
						{#each actions as a (a)}
							<span
								class="inline-flex items-center gap-1 rounded-md bg-muted px-2 py-1 font-mono text-xs"
							>
								{a}
								<button
									type="button"
									class="text-muted-foreground hover:text-foreground"
									aria-label="Remove {a}"
									onclick={() => removeAction(a)}
								>
									<X class="size-3" />
								</button>
							</span>
						{/each}
					</div>
				{/if}
			</div>

			<div class="flex flex-col gap-2">
				<div class="flex items-center justify-between">
					<Label for="sim-context-add" class="text-xs">Context entries</Label>
					<Button
						id="sim-context-add"
						type="button"
						variant="ghost"
						size="xs"
						onclick={addContext}
					>
						<Plus class="size-3" /> Add context
					</Button>
				</div>
				{#each contextEntries as entry, i (i)}
					<div class="grid grid-cols-12 gap-2">
						<Input
							placeholder="aws:SourceIp"
							bind:value={entry.key}
							class="col-span-4 h-8 font-mono text-xs"
						/>
						<Input
							placeholder="value"
							bind:value={entry.values[0]}
							class="col-span-5 h-8 font-mono text-xs"
						/>
						<select
							bind:value={entry.type}
							class="col-span-2 h-8 rounded-md border border-input bg-background px-2 text-xs"
						>
							<option value="string">string</option>
							<option value="stringList">stringList</option>
							<option value="numeric">numeric</option>
							<option value="boolean">boolean</option>
							<option value="ip">ip</option>
							<option value="date">date</option>
						</select>
						<Button
							type="button"
							variant="ghost"
							size="icon-sm"
							class="col-span-1"
							aria-label="Remove context"
							onclick={() => removeContext(i)}
						>
							<X class="size-3" />
						</Button>
					</div>
				{/each}
			</div>

			<div class="flex justify-end">
				<Button onclick={runPrincipal} disabled={principalLoading}>
					{#if principalLoading}
						<Loader2 class="size-4 animate-spin" />
					{:else}
						<Play class="size-4" />
					{/if}
					Run simulation
				</Button>
			</div>
		</CardContent>
	</Card>

	<Card>
		<CardHeader>
			<CardTitle class="flex items-center gap-2">
				<Sparkles class="size-4 text-primary" /> Simulate custom policy
			</CardTitle>
		</CardHeader>
		<CardContent class="grid gap-4">
			<PolicyEditor bind:value={customPolicy} id="custom-policy" rows={12} />
			<div class="grid gap-3 md:grid-cols-2">
				<div class="flex flex-col gap-1.5">
					<Label for="custom-action" class="text-xs">Action</Label>
					<Input
						id="custom-action"
						bind:value={customAction}
						placeholder="s3:GetObject"
						class="font-mono text-xs"
					/>
				</div>
				<div class="flex flex-col gap-1.5">
					<Label for="custom-resource" class="text-xs">Resource ARN</Label>
					<Input
						id="custom-resource"
						bind:value={customResource}
						placeholder="arn:aws:s3:::bucket/key"
						class="font-mono text-xs"
					/>
				</div>
			</div>
			<div class="flex justify-end">
				<Button onclick={runCustom} disabled={customLoading} variant="outline">
					{#if customLoading}
						<Loader2 class="size-4 animate-spin" />
					{:else}
						<Play class="size-4" />
					{/if}
					Run custom simulation
				</Button>
			</div>
			{#if customResult}
				<div class="space-y-2">
					{#each customResult.results as r (r.evalActionName)}
						{@const allowed = r.evalDecision === 'allowed'}
						<div
							class="flex items-center gap-3 rounded-md border p-3 {allowed
								? 'border-emerald-500/40 bg-emerald-500/5'
								: 'border-destructive/40 bg-destructive/5'}"
						>
							{#if allowed}
								<ShieldCheck class="size-6 text-emerald-500" />
							{:else}
								<ShieldX class="size-6 text-destructive" />
							{/if}
							<div class="min-w-0 flex-1">
								<div class="font-mono text-xs">{r.evalActionName}</div>
								<div
									class="text-base font-semibold {allowed
										? 'text-emerald-500'
										: 'text-destructive'}"
								>
									{decisionLabel(r.evalDecision)}
								</div>
							</div>
						</div>
					{/each}
				</div>
			{/if}
		</CardContent>
	</Card>
</div>
