<script lang="ts">
	// Reusable API-key credentials. A single credential can be
	// referenced by N backends via `backend.credential = "<name>"`,
	// so a fan-out setup doesn't have to restate the same secret in
	// every backend block.
	//
	// Reads + writes the live runtime config; the server validates
	// on apply (missing/duplicate names, env-var resolution, conflict
	// with a backend's legacy api_key fields).
	import { onMount } from 'svelte';
	import {
		getRuntimeConfig,
		putRuntimeConfig,
		type BedrockSpec,
		type CredentialSpec,
		type RuntimeConfig,
		type RuntimeConfigEnvelope,
	} from '$lib/api/runtime-config';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import {
		Select,
		SelectContent,
		SelectItem,
		SelectTrigger,
	} from '$lib/components/ui/select';
	import {
		Table,
		TableBody,
		TableCell,
		TableHead,
		TableHeader,
		TableRow,
	} from '$lib/components/ui/table';
	import { EmptyState } from '$lib/components/service';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import SaveIcon from '@lucide/svelte/icons/save';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import { toast } from 'svelte-sonner';

	interface CredentialRow {
		name: string;
		mode: 'inline' | 'env';
		apiKey: string;
		apiKeyEnv: string;
	}

	let envelope = $state<RuntimeConfigEnvelope | null>(null);
	let rows = $state<CredentialRow[]>([]);
	let originalSnapshot = $state<string>('');
	let loading = $state(true);
	let saving = $state(false);

	onMount(load);

	async function load() {
		loading = true;
		try {
			envelope = await getRuntimeConfig();
			rows = credentialsToRows(envelope.config.bedrock.spec.credentials ?? {});
			originalSnapshot = JSON.stringify(rows);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load credentials');
		} finally {
			loading = false;
		}
	}

	function credentialsToRows(creds: Record<string, CredentialSpec>): CredentialRow[] {
		return Object.entries(creds)
			.map(([name, c]) => ({
				name,
				mode: (c.api_key_env ? 'env' : 'inline') as 'inline' | 'env',
				apiKey: c.api_key ?? '',
				apiKeyEnv: c.api_key_env ?? '',
			}))
			.sort((a, b) => a.name.localeCompare(b.name));
	}

	function rowsToCredentials(): Record<string, CredentialSpec> {
		const out: Record<string, CredentialSpec> = {};
		for (const r of rows) {
			const n = r.name.trim();
			if (!n) continue;
			out[n] = r.mode === 'inline' ? { api_key: r.apiKey } : { api_key_env: r.apiKeyEnv };
		}
		return out;
	}

	// Walk the live backends spec to count references to each
	// credential name. Surfaces as a chip per row so users can see
	// at a glance whether removing a credential would orphan a
	// backend (and the server will refuse the apply if it does).
	let usedBy = $derived.by<Record<string, string[]>>(() => {
		const out: Record<string, string[]> = {};
		const backends = envelope?.config.bedrock.spec.backends ?? {};
		for (const [bname, b] of Object.entries(backends)) {
			if (b.credential) {
				(out[b.credential] ??= []).push(bname);
			}
		}
		return out;
	});

	let isModified = $derived(originalSnapshot !== JSON.stringify(rows));

	let validationError = $derived.by<string | null>(() => {
		const names = new Set<string>();
		for (const r of rows) {
			const n = r.name.trim();
			if (!n) return 'Every credential needs a name.';
			if (names.has(n)) return `Duplicate credential name: '${n}'.`;
			names.add(n);
			if (r.mode === 'inline' && r.apiKey.trim().length === 0) {
				return `Credential '${n}' is set to inline but has no API key.`;
			}
			if (r.mode === 'env' && r.apiKeyEnv.trim().length === 0) {
				return `Credential '${n}' is set to env var but has no variable name.`;
			}
		}
		return null;
	});

	function addRow() {
		const base = 'credential';
		const existing = new Set(rows.map((r) => r.name));
		let n = base;
		let i = 2;
		while (existing.has(n)) {
			n = `${base}-${i++}`;
		}
		rows = [...rows, { name: n, mode: 'env', apiKey: '', apiKeyEnv: '' }];
	}

	function removeRow(i: number) {
		const r = rows[i];
		const refs = usedBy[r.name] ?? [];
		if (refs.length > 0) {
			toast.error(
				`Credential '${r.name}' is referenced by ${refs.length} backend${refs.length === 1 ? '' : 's'} (${refs.join(', ')}). Update those first.`,
			);
			return;
		}
		rows = rows.filter((_, j) => j !== i);
	}

	function resetRows() {
		if (!envelope) return;
		rows = credentialsToRows(envelope.config.bedrock.spec.credentials ?? {});
	}

	async function save() {
		if (!envelope) return;
		if (validationError) {
			toast.error(validationError);
			return;
		}
		saving = true;
		try {
			const next: RuntimeConfig = {
				...envelope.config,
				bedrock: {
					...envelope.config.bedrock,
					spec: {
						...envelope.config.bedrock.spec,
						credentials: rowsToCredentials(),
					} satisfies BedrockSpec,
				},
			};
			const updated = await putRuntimeConfig(next);
			envelope = updated;
			rows = credentialsToRows(updated.config.bedrock.spec.credentials ?? {});
			originalSnapshot = JSON.stringify(rows);
			toast.success('Credentials saved');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Save failed');
		} finally {
			saving = false;
		}
	}
</script>

<div class="space-y-4 p-4">
	<section class="rounded-lg border bg-card p-4 text-sm">
		<div class="flex items-start gap-3">
			<KeyRound class="mt-0.5 h-5 w-5 shrink-0 text-muted-foreground" />
			<div class="space-y-1">
				<p class="font-semibold">Reusable credentials</p>
				<p class="text-muted-foreground">
					Define an API key once and reference it from multiple backends via
					<code>credential = "name"</code>. Env-var mode is preferred so secrets
					stay out of the on-disk config; inline mode works too but the value
					lives in <code>runtime-config.json</code> verbatim. The backend picker
					arrives with the Add Backend wizard in Phase 2; meanwhile, you can wire
					backend.credential by editing the TOML directly or via the Settings page.
				</p>
			</div>
		</div>
	</section>

	<header class="flex items-center justify-between">
		<div>
			<h2 class="text-lg font-semibold">Credentials</h2>
			<p class="text-sm text-muted-foreground">
				{rows.length} credential{rows.length === 1 ? '' : 's'}
				{#if isModified}<span class="ml-2 text-amber-600">unsaved changes</span>{/if}
			</p>
		</div>
		<div class="flex gap-2">
			<Button variant="ghost" size="sm" onclick={load} disabled={loading || saving}>
				<RefreshCwIcon class={loading ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />
				<span class="ml-2">Reload</span>
			</Button>
			{#if isModified}
				<Button variant="ghost" size="sm" onclick={resetRows} disabled={saving}>
					<RotateCcwIcon class="h-4 w-4" />
					<span class="ml-2">Reset</span>
				</Button>
			{/if}
			<Button variant="outline" size="sm" onclick={addRow} disabled={loading || saving}>
				<PlusIcon class="h-4 w-4" />
				<span class="ml-2">Add credential</span>
			</Button>
			<Button size="sm" onclick={save} disabled={loading || saving || !isModified}>
				<SaveIcon class="h-4 w-4" />
				<span class="ml-2">{saving ? 'Saving…' : 'Save'}</span>
			</Button>
		</div>
	</header>

	{#if validationError && isModified}
		<div
			class="flex items-start gap-2 rounded border border-amber-500/30 bg-amber-500/5 p-3 text-sm"
		>
			<CircleAlertIcon class="mt-0.5 h-4 w-4 shrink-0 text-amber-600" />
			<p>{validationError}</p>
		</div>
	{/if}

	{#if loading && rows.length === 0}
		<EmptyState icon={KeyRound} title="Loading credentials…" />
	{:else if rows.length === 0}
		<EmptyState
			icon={KeyRound}
			title="No credentials yet"
			description="Add a credential to share one API key across multiple backends. Backends can also keep using inline api_key / api_key_env fields for back-compat."
		/>
	{:else}
		<div class="rounded-lg border bg-card">
			<Table>
				<TableHeader>
					<TableRow>
						<TableHead>Name</TableHead>
						<TableHead>Mode</TableHead>
						<TableHead>Value</TableHead>
						<TableHead>Used by</TableHead>
						<TableHead class="text-right">Actions</TableHead>
					</TableRow>
				</TableHeader>
				<TableBody>
					{#each rows as row, i (i)}
						{@const refs = usedBy[row.name] ?? []}
						{@const inUse = refs.length > 0}
						<TableRow>
							<TableCell class="align-top">
								<Label class="sr-only" for={`cred-name-${i}`}>Name</Label>
								<Input
									id={`cred-name-${i}`}
									bind:value={row.name}
									placeholder="e.g. groq"
									class="font-mono text-sm"
								/>
							</TableCell>
							<TableCell class="align-top">
								<Select
									type="single"
									value={row.mode}
									onValueChange={(v) => (row.mode = v as CredentialRow['mode'])}
								>
									<SelectTrigger class="w-32">
										{row.mode === 'inline' ? 'Inline key' : 'Env var'}
									</SelectTrigger>
									<SelectContent>
										<SelectItem value="env" label="Env var">Env var</SelectItem>
										<SelectItem value="inline" label="Inline key">Inline key</SelectItem>
									</SelectContent>
								</Select>
							</TableCell>
							<TableCell class="align-top">
								{#if row.mode === 'inline'}
									<Input
										type="password"
										bind:value={row.apiKey}
										placeholder="sk-…"
										class="font-mono text-sm"
									/>
								{:else}
									<Input
										bind:value={row.apiKeyEnv}
										placeholder="GROQ_API_KEY"
										class="font-mono text-sm"
									/>
								{/if}
							</TableCell>
							<TableCell class="align-top">
								{#if inUse}
									<div class="flex flex-wrap gap-1">
										{#each refs as b (b)}
											<Badge variant="secondary" class="font-mono text-xs">{b}</Badge>
										{/each}
									</div>
								{:else}
									<span class="text-xs text-muted-foreground">—</span>
								{/if}
							</TableCell>
							<TableCell class="text-right align-top">
								<Button
									variant="ghost"
									size="icon"
									onclick={() => removeRow(i)}
									aria-label="Remove"
									disabled={inUse}
									title={inUse
										? `In use by ${refs.length} backend${refs.length === 1 ? '' : 's'}`
										: 'Remove credential'}
								>
									<Trash2Icon class="h-4 w-4" />
								</Button>
							</TableCell>
						</TableRow>
					{/each}
				</TableBody>
			</Table>
		</div>
	{/if}
</div>
