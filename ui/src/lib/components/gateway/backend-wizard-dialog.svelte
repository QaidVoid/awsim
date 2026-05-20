<script lang="ts">
	// Two-step Add Backend wizard: pick a provider from the bundled
	// catalog, then fill in name/endpoint/auth with sensible defaults
	// driven by the provider's catalog entry. Reused for Edit, where
	// we skip step 1 and pre-fill step 2 from the existing backend.
	//
	// The dialog is pure UI: it asks the parent to apply via onSubmit
	// so the parent owns the runtime-config GET/PUT round-trip.
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
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
	import { Separator } from '$lib/components/ui/separator';
	import type { CatalogProvider } from '$lib/api/gateway';
	import type { BedrockBackendSpec } from '$lib/api/runtime-config';
	import type { Component } from 'svelte';

	import Server from '@lucide/svelte/icons/server';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Zap from '@lucide/svelte/icons/zap';
	import Flame from '@lucide/svelte/icons/flame';
	import RouteIcon from '@lucide/svelte/icons/route';
	import Cloud from '@lucide/svelte/icons/cloud';
	import Settings from '@lucide/svelte/icons/settings';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';

	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	const ICON_MAP: Record<string, Component<any>> = {
		server: Server,
		sparkles: Sparkles,
		zap: Zap,
		flame: Flame,
		route: RouteIcon,
		cloud: Cloud,
		settings: Settings,
	};

	function iconFor(name: string) {
		return ICON_MAP[name] ?? Server;
	}

	interface InitialBackend {
		name: string;
		spec: BedrockBackendSpec;
	}

	interface Props {
		open: boolean;
		mode: 'add' | 'edit';
		providers: CatalogProvider[];
		credentials: string[];
		existingNames: string[];
		/** Required when mode === 'edit'; ignored otherwise. */
		initial?: InitialBackend | null;
		onOpenChange: (open: boolean) => void;
		onSubmit: (result: InitialBackend) => void | Promise<void>;
	}

	let {
		open,
		mode,
		providers,
		credentials,
		existingNames,
		initial = null,
		onOpenChange,
		onSubmit,
	}: Props = $props();

	type AuthMode = 'none' | 'credential' | 'inline' | 'env';

	let step = $state<1 | 2>(1);
	let selectedProvider = $state<CatalogProvider | null>(null);
	let name = $state('');
	let endpoint = $state('');
	let authMode = $state<AuthMode>('env');
	let credentialRef = $state('');
	let apiKey = $state('');
	let apiKeyEnv = $state('');
	let submitting = $state(false);

	// Track the most recent (open, mode, initial.name) so we re-seed
	// the form when the parent reopens the dialog. We can't rely on
	// `onMount` because the same instance is reused across opens.
	let lastOpenKey = $state('');

	$effect(() => {
		const key = `${open}|${mode}|${initial?.name ?? ''}`;
		if (key !== lastOpenKey && open) {
			lastOpenKey = key;
			seedForm();
		}
	});

	function seedForm() {
		submitting = false;
		if (mode === 'edit' && initial) {
			const providerKey = initial.spec.provider ?? '';
			const found = providers.find((p) => p.key === providerKey);
			selectedProvider = found ?? customProvider();
			step = 2;
			name = initial.name;
			endpoint = initial.spec.endpoint;
			credentialRef = initial.spec.credential ?? '';
			apiKey = initial.spec.api_key ?? '';
			apiKeyEnv = initial.spec.api_key_env ?? '';
			authMode = inferAuthMode(initial.spec, selectedProvider);
		} else {
			selectedProvider = null;
			step = 1;
			name = '';
			endpoint = '';
			credentialRef = '';
			apiKey = '';
			apiKeyEnv = '';
			authMode = 'env';
		}
	}

	function inferAuthMode(spec: BedrockBackendSpec, prov: CatalogProvider | null): AuthMode {
		if (spec.credential) return 'credential';
		if (spec.api_key) return 'inline';
		if (spec.api_key_env) return 'env';
		return prov?.auth === 'none' ? 'none' : 'env';
	}

	function customProvider(): CatalogProvider {
		// Fallback when the spec's provider key isn't in the catalog
		// (legacy backend, or user removed a catalog entry). Treat as
		// the generic Custom slot so the form still works.
		const c = providers.find((p) => p.key === 'custom');
		return (
			c ?? {
				key: 'custom',
				name: 'Custom',
				icon: 'settings',
				kind: 'custom',
				endpoint_template: '',
				auth: 'bearer',
				env_hint: null,
				docs_url: null,
				notes: null,
				models: [],
			}
		);
	}

	function pickProvider(p: CatalogProvider) {
		selectedProvider = p;
		// Suggest a name + endpoint based on the provider. User can
		// override either before saving.
		if (mode === 'add') {
			name = uniqueName(p.key);
			endpoint = p.endpoint_template;
			if (p.auth === 'none') {
				authMode = 'none';
				apiKey = '';
				apiKeyEnv = '';
			} else {
				authMode = 'env';
				apiKeyEnv = p.env_hint ?? '';
			}
		}
		step = 2;
	}

	function uniqueName(base: string): string {
		if (!existingNames.includes(base)) return base;
		let i = 2;
		while (existingNames.includes(`${base}-${i}`)) i++;
		return `${base}-${i}`;
	}

	let nameError = $derived.by<string | null>(() => {
		const n = name.trim();
		if (!n) return 'Name is required.';
		if (!/^[a-zA-Z0-9._-]+$/.test(n)) {
			return 'Use letters, digits, dot, dash, or underscore only.';
		}
		const taken = existingNames.includes(n) && !(mode === 'edit' && initial && initial.name === n);
		if (taken) return `A backend named '${n}' already exists.`;
		return null;
	});

	let endpointError = $derived.by<string | null>(() => {
		const e = endpoint.trim();
		if (!e) return 'Endpoint is required.';
		if (!/^https?:\/\//.test(e)) return 'Endpoint must start with http:// or https://.';
		return null;
	});

	let authError = $derived.by<string | null>(() => {
		switch (authMode) {
			case 'credential':
				if (!credentialRef) return 'Pick a credential or change auth mode.';
				if (!credentials.includes(credentialRef)) {
					return `Credential '${credentialRef}' no longer exists.`;
				}
				return null;
			case 'inline':
				return apiKey.trim() ? null : 'Inline mode needs an API key value.';
			case 'env':
				return apiKeyEnv.trim() ? null : 'Env mode needs a variable name.';
			case 'none':
				return null;
		}
	});

	let canSubmit = $derived(
		step === 2 && !nameError && !endpointError && !authError && !submitting,
	);

	async function submit() {
		if (!canSubmit || !selectedProvider) return;
		submitting = true;
		const spec: BedrockBackendSpec = {
			endpoint: endpoint.trim(),
			provider: selectedProvider.key === 'custom' ? 'custom' : selectedProvider.key,
		};
		switch (authMode) {
			case 'credential':
				spec.credential = credentialRef;
				break;
			case 'inline':
				spec.api_key = apiKey;
				break;
			case 'env':
				spec.api_key_env = apiKeyEnv.trim();
				break;
			case 'none':
				// nothing
				break;
		}
		try {
			await onSubmit({ name: name.trim(), spec });
			onOpenChange(false);
		} finally {
			submitting = false;
		}
	}

	function back() {
		if (mode === 'edit') return;
		step = 1;
	}

	function close() {
		onOpenChange(false);
	}

	function authLabel(m: AuthMode): string {
		switch (m) {
			case 'none':
				return 'No auth';
			case 'credential':
				return 'Reuse credential';
			case 'inline':
				return 'Inline key';
			case 'env':
				return 'Env var';
		}
	}
</script>

<Dialog {open} onOpenChange={(o) => onOpenChange(o)}>
	<DialogContent class="sm:max-w-2xl">
		<DialogHeader>
			<DialogTitle>
				{mode === 'edit' ? 'Edit backend' : step === 1 ? 'Pick a provider' : 'Configure backend'}
			</DialogTitle>
			<DialogDescription>
				{#if step === 1}
					Templates pre-fill the endpoint, auth field, and a curated model list. Pick "Custom" for anything OpenAI-compatible that isn't in the catalog.
				{:else if selectedProvider}
					{selectedProvider.name} — {selectedProvider.notes ?? 'OpenAI-compatible backend.'}
				{:else}
					&nbsp;
				{/if}
			</DialogDescription>
		</DialogHeader>

		{#if step === 1}
			<div class="grid max-h-[60vh] grid-cols-1 gap-2 overflow-y-auto px-4 sm:grid-cols-2">
				{#each providers as p (p.key)}
					{@const Icon = iconFor(p.icon)}
					<button
						type="button"
						class="flex items-start gap-3 rounded-lg border bg-card p-3 text-left transition hover:border-primary hover:bg-accent"
						onclick={() => pickProvider(p)}
					>
						<Icon class="mt-0.5 h-5 w-5 shrink-0 text-muted-foreground" />
						<div class="min-w-0 flex-1 space-y-1">
							<div class="flex items-center gap-2">
								<span class="font-medium leading-tight">{p.name}</span>
								<Badge
									variant={p.kind === 'aws' ? 'default' : p.kind === 'hosted' ? 'secondary' : 'outline'}
									class="text-[10px] uppercase"
								>
									{p.kind}
								</Badge>
							</div>
							<div class="truncate font-mono text-xs text-muted-foreground">
								{p.endpoint_template || '(no template)'}
							</div>
							{#if p.env_hint}
								<div class="font-mono text-xs text-muted-foreground">${p.env_hint}</div>
							{/if}
						</div>
					</button>
				{/each}
			</div>
		{:else if selectedProvider}
			<div class="flex flex-col gap-4 px-4">
				{#if mode === 'add'}
					<button
						type="button"
						class="-ml-2 flex w-fit items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
						onclick={back}
					>
						<ChevronLeftIcon class="h-3.5 w-3.5" />
						Change provider ({selectedProvider.name})
					</button>
				{/if}

				<div class="flex flex-col gap-1">
					<Label for="backend-name">Name</Label>
					<Input
						id="backend-name"
						bind:value={name}
						placeholder="ollama"
						autocomplete="off"
						class="font-mono"
					/>
					{#if nameError && name}
						<p class="text-xs text-amber-600">{nameError}</p>
					{/if}
				</div>

				<div class="flex flex-col gap-1">
					<Label for="backend-endpoint">Endpoint</Label>
					<Input
						id="backend-endpoint"
						bind:value={endpoint}
						placeholder={selectedProvider.endpoint_template}
						autocomplete="off"
						class="font-mono"
					/>
					{#if endpointError && endpoint}
						<p class="text-xs text-amber-600">{endpointError}</p>
					{/if}
				</div>

				{#if selectedProvider.auth === 'none'}
					<div class="rounded border bg-muted/30 p-3 text-xs text-muted-foreground">
						{selectedProvider.name} doesn't require an API key.
					</div>
				{:else}
					<Separator />

					<div class="flex flex-col gap-2">
						<Label>Authentication</Label>
						<Select
							type="single"
							value={authMode}
							onValueChange={(v) => (authMode = v as AuthMode)}
						>
							<SelectTrigger class="w-full">
								{authLabel(authMode)}
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="env" label="Env var">Env var</SelectItem>
								<SelectItem value="credential" label="Reuse credential">
									Reuse credential
								</SelectItem>
								<SelectItem value="inline" label="Inline key">Inline key</SelectItem>
								<SelectItem value="none" label="No auth">No auth</SelectItem>
							</SelectContent>
						</Select>

						{#if authMode === 'credential'}
							{#if credentials.length === 0}
								<div
									class="flex items-start gap-2 rounded border border-amber-500/30 bg-amber-500/5 p-2 text-xs"
								>
									<CircleAlertIcon class="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-600" />
									<p>
										No credentials defined yet. Switch to Env var / Inline, or define one in the
										Credentials tab first.
									</p>
								</div>
							{:else}
								<Select
									type="single"
									value={credentialRef}
									onValueChange={(v) => (credentialRef = v)}
								>
									<SelectTrigger class="w-full font-mono">
										{credentialRef || '(pick a credential)'}
									</SelectTrigger>
									<SelectContent>
										{#each credentials as c (c)}
											<SelectItem value={c} label={c}>{c}</SelectItem>
										{/each}
									</SelectContent>
								</Select>
							{/if}
						{:else if authMode === 'inline'}
							<Input
								type="password"
								bind:value={apiKey}
								placeholder="sk-…"
								class="font-mono"
							/>
							<p class="text-xs text-muted-foreground">
								Stored in plain text in runtime-config.json. Prefer Env var or a reusable
								Credential for shared machines.
							</p>
						{:else if authMode === 'env'}
							<Input
								bind:value={apiKeyEnv}
								placeholder={selectedProvider.env_hint ?? 'PROVIDER_API_KEY'}
								class="font-mono"
							/>
							<p class="text-xs text-muted-foreground">
								Resolved against the awsim process environment at apply time. Save fails if the
								variable is unset.
							</p>
						{/if}

						{#if authError && authMode !== 'none'}
							<p class="text-xs text-amber-600">{authError}</p>
						{/if}
					</div>
				{/if}
			</div>
		{/if}

		<DialogFooter>
			<Button variant="outline" onclick={close}>Cancel</Button>
			{#if step === 2}
				<Button onclick={submit} disabled={!canSubmit}>
					{submitting ? 'Saving…' : mode === 'edit' ? 'Save changes' : 'Add backend'}
				</Button>
			{/if}
		</DialogFooter>
	</DialogContent>
</Dialog>
