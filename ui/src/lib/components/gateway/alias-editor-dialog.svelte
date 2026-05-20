<script lang="ts">
	// Alias editor: pick a Bedrock id, pick chat/embed, then build an
	// ordered list of (backend, tag) targets. The first target whose
	// backend exists wins at runtime (Phase 3 = First strategy);
	// per-target overrides and runtime fallback land in Phases 4-5.
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
	import type {
		AliasKind,
		AliasSpec,
		AliasTarget,
	} from '$lib/api/runtime-config';
	import type { CatalogProvider, ProviderCatalog } from '$lib/api/gateway';

	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import SlidersIcon from '@lucide/svelte/icons/sliders-horizontal';

	interface BackendOption {
		name: string;
		providerKey: string | null;
	}

	interface InitialAlias {
		id: string;
		alias: AliasSpec;
	}

	interface Props {
		open: boolean;
		mode: 'add' | 'edit';
		backends: BackendOption[];
		catalog: ProviderCatalog | null;
		existingIds: string[];
		initial?: InitialAlias | null;
		onOpenChange: (open: boolean) => void;
		onSubmit: (result: InitialAlias) => void | Promise<void>;
	}

	let {
		open,
		mode,
		backends,
		catalog,
		existingIds,
		initial = null,
		onOpenChange,
		onSubmit,
	}: Props = $props();

	interface TargetRow extends AliasTarget {
		// Stable client-side key for keyed each-blocks across
		// reorders. Bumped on add/remove so Svelte doesn't reuse
		// node identity across positions.
		_k: number;
		// UI-local toggle for the per-target overrides panel; not
		// serialised back to the wire shape.
		_overridesOpen: boolean;
	}

	let bedrockId = $state('');
	let kind = $state<AliasKind>('chat');
	let targets = $state<TargetRow[]>([]);
	let submitting = $state(false);
	// Re-seed every closed -> open transition. Without this, stale
	// text from a previous Cancel persists when the user reopens
	// with the same mode/initial (since `mode` + `initial?.id`
	// don't change in that case).
	let wasOpen = $state(false);
	let rowKeyCounter = 1;

	$effect(() => {
		if (open && !wasOpen) {
			seed();
		}
		wasOpen = open;
	});

	function seed() {
		submitting = false;
		if (mode === 'edit' && initial) {
			bedrockId = initial.id;
			kind = initial.alias.kind ?? 'chat';
			targets = initial.alias.targets.map((t) => ({
				...t,
				_k: rowKeyCounter++,
				// Auto-open the overrides panel when the target
				// already has one set, so users see + can edit it.
				_overridesOpen:
					(t.timeout_ms ?? null) !== null ||
					(t.max_tokens ?? null) !== null ||
					(t.temperature ?? null) !== null,
			}));
		} else {
			bedrockId = '';
			kind = 'chat';
			// Seed with one empty target so the user lands on a fillable
			// row instead of an empty list with a Add-target button.
			targets = [
				{
					backend: backends[0]?.name ?? '',
					tag: '',
					_k: rowKeyCounter++,
					_overridesOpen: false,
				},
			];
		}
	}

	// Suggested Bedrock ids pulled from the catalog's `bedrock`
	// entry. Surfaces as a datalist beneath the input so the user
	// can either pick a known id or type a custom one.
	let suggestedIds = $derived.by<string[]>(() => {
		if (!catalog) return [];
		const wantedKind = kind;
		const bedrockProv = catalog.providers.find((p) => p.key === 'bedrock');
		if (!bedrockProv) return [];
		return bedrockProv.models.filter((m) => m.kind === wantedKind).map((m) => m.id);
	});

	function providerForBackend(name: string): CatalogProvider | null {
		if (!catalog) return null;
		const bo = backends.find((b) => b.name === name);
		if (!bo?.providerKey) return null;
		return catalog.providers.find((p) => p.key === bo.providerKey) ?? null;
	}

	function suggestedTagsFor(backendName: string): string[] {
		const p = providerForBackend(backendName);
		if (!p) return [];
		return p.models.filter((m) => m.kind === kind).map((m) => m.id);
	}

	function addTarget() {
		targets = [
			...targets,
			{
				backend: backends[0]?.name ?? '',
				tag: '',
				_k: rowKeyCounter++,
				_overridesOpen: false,
			},
		];
	}

	function removeTarget(i: number) {
		targets = targets.filter((_, j) => j !== i);
	}

	function moveUp(i: number) {
		if (i === 0) return;
		const next = targets.slice();
		[next[i - 1], next[i]] = [next[i], next[i - 1]];
		targets = next;
	}

	function moveDown(i: number) {
		if (i === targets.length - 1) return;
		const next = targets.slice();
		[next[i], next[i + 1]] = [next[i + 1], next[i]];
		targets = next;
	}

	let idError = $derived.by<string | null>(() => {
		const id = bedrockId.trim();
		if (!id) return 'Bedrock id is required.';
		const taken = existingIds.includes(id) && !(mode === 'edit' && initial && initial.id === id);
		if (taken) return `An alias for '${id}' already exists.`;
		return null;
	});

	let targetsError = $derived.by<string | null>(() => {
		if (targets.length === 0) return 'Add at least one target.';
		for (let i = 0; i < targets.length; i++) {
			const t = targets[i];
			if (!t.backend) return `Target ${i + 1}: pick a backend.`;
			if (!backends.some((b) => b.name === t.backend)) {
				return `Target ${i + 1} references unknown backend '${t.backend}'.`;
			}
			if (!t.tag.trim()) return `Target ${i + 1}: model tag is required.`;
		}
		return null;
	});

	let canSubmit = $derived(!idError && !targetsError && !submitting);

	async function submit() {
		if (!canSubmit) return;
		submitting = true;
		const alias: AliasSpec = {
			kind,
			strategy: 'first',
			targets: targets.map((t) => {
				const out: AliasTarget = { backend: t.backend, tag: t.tag.trim() };
				if ((t.timeout_ms ?? null) !== null) out.timeout_ms = t.timeout_ms ?? undefined;
				// Chat-only overrides: drop on embed so they don't
				// land in the on-disk config and confuse future readers.
				if (kind === 'chat') {
					if ((t.max_tokens ?? null) !== null) out.max_tokens = t.max_tokens ?? undefined;
					if ((t.temperature ?? null) !== null) out.temperature = t.temperature ?? undefined;
				}
				return out;
			}),
		};
		try {
			await onSubmit({ id: bedrockId.trim(), alias });
			onOpenChange(false);
		} finally {
			submitting = false;
		}
	}

	function close() {
		onOpenChange(false);
	}
</script>

<Dialog {open} onOpenChange={(o) => onOpenChange(o)}>
	<DialogContent class="sm:max-w-2xl">
		<DialogHeader>
			<DialogTitle>
				{mode === 'edit' ? 'Edit alias' : 'Add mapping'}
			</DialogTitle>
			<DialogDescription>
				Map a Bedrock model id to an ordered list of backend targets. The first target whose backend exists wins at runtime.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-4 px-4">
			<div class="grid grid-cols-1 gap-3 sm:grid-cols-[1fr_140px]">
				<div class="flex flex-col gap-1">
					<Label for="alias-bedrock-id">Bedrock model id</Label>
					<Input
						id="alias-bedrock-id"
						list="alias-bedrock-id-suggestions"
						bind:value={bedrockId}
						placeholder="anthropic.claude-3-5-sonnet-20241022-v2:0"
						autocomplete="off"
						class="font-mono"
						disabled={mode === 'edit'}
					/>
					<datalist id="alias-bedrock-id-suggestions">
						{#each suggestedIds as id (id)}
							<option value={id}></option>
						{/each}
					</datalist>
					{#if idError && bedrockId}
						<p class="text-xs text-amber-600">{idError}</p>
					{/if}
				</div>
				<div class="flex flex-col gap-1">
					<Label>Kind</Label>
					<Select
						type="single"
						value={kind}
						onValueChange={(v) => (kind = v as AliasKind)}
						disabled={mode === 'edit'}
					>
						<SelectTrigger class="w-full">{kind}</SelectTrigger>
						<SelectContent>
							<SelectItem value="chat" label="chat">chat</SelectItem>
							<SelectItem value="embed" label="embed">embed</SelectItem>
						</SelectContent>
					</Select>
				</div>
			</div>

			<Separator />

			<div class="flex items-center justify-between">
				<Label>Targets <Badge variant="outline" class="ml-2 text-[10px]">first strategy</Badge></Label>
				<Button variant="outline" size="sm" onclick={addTarget}>
					<PlusIcon class="h-3.5 w-3.5" />
					<span class="ml-1">Add target</span>
				</Button>
			</div>

			{#if backends.length === 0}
				<div
					class="flex items-start gap-2 rounded border border-amber-500/30 bg-amber-500/5 p-3 text-xs"
				>
					<CircleAlertIcon class="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-600" />
					<p>
						No backends are configured yet. Add one in the Backends tab before wiring an alias —
						the resolver needs a real backend to route to.
					</p>
				</div>
			{:else}
				<div class="flex flex-col gap-2">
					{#each targets as t, i (t._k)}
						{@const suggestions = suggestedTagsFor(t.backend)}
						{@const hasOverrides =
							(t.timeout_ms ?? null) !== null ||
							(t.max_tokens ?? null) !== null ||
							(t.temperature ?? null) !== null}
						<div class="flex flex-col gap-2 rounded border p-2">
							<div class="grid grid-cols-[auto_1fr_1fr_auto] items-end gap-2">
								<div class="flex flex-col gap-1">
									<Label class="text-[10px] uppercase text-muted-foreground">Priority</Label>
									<div class="flex h-9 w-12 items-center justify-center rounded bg-muted/40 font-mono text-sm">
										#{i + 1}
									</div>
								</div>
								<div class="flex flex-col gap-1">
									<Label class="text-[10px] uppercase text-muted-foreground">Backend</Label>
									<Select
										type="single"
										value={t.backend}
										onValueChange={(v) => (t.backend = v)}
									>
										<SelectTrigger class="w-full font-mono">
											{t.backend || '(pick)'}
										</SelectTrigger>
										<SelectContent>
											{#each backends as b (b.name)}
												<SelectItem value={b.name} label={b.name}>{b.name}</SelectItem>
											{/each}
										</SelectContent>
									</Select>
								</div>
								<div class="flex flex-col gap-1">
									<Label class="text-[10px] uppercase text-muted-foreground">Model tag</Label>
									<Input
										list={`alias-tag-suggestions-${t._k}`}
										bind:value={t.tag}
										placeholder="llama3.1:8b"
										autocomplete="off"
										class="font-mono"
									/>
									<datalist id={`alias-tag-suggestions-${t._k}`}>
										{#each suggestions as s (s)}
											<option value={s}></option>
										{/each}
									</datalist>
								</div>
								<div class="flex gap-0">
									<Button
										variant="ghost"
										size="icon"
										onclick={() => moveUp(i)}
										disabled={i === 0}
										aria-label="Move up"
									>
										<ArrowUpIcon class="h-4 w-4" />
									</Button>
									<Button
										variant="ghost"
										size="icon"
										onclick={() => moveDown(i)}
										disabled={i === targets.length - 1}
										aria-label="Move down"
									>
										<ArrowDownIcon class="h-4 w-4" />
									</Button>
									<Button
										variant="ghost"
										size="icon"
										onclick={() => removeTarget(i)}
										disabled={targets.length === 1}
										aria-label="Remove target"
									>
										<Trash2Icon class="h-4 w-4" />
									</Button>
								</div>
							</div>

							<button
								type="button"
								class="flex items-center justify-between text-xs text-muted-foreground hover:text-foreground"
								onclick={() => (t._overridesOpen = !t._overridesOpen)}
							>
								<span class="flex items-center gap-1">
									<SlidersIcon class="h-3 w-3" />
									Per-target overrides
									{#if hasOverrides}
										<Badge variant="secondary" class="ml-1 text-[9px]">customised</Badge>
									{/if}
								</span>
								<span>{t._overridesOpen ? 'hide' : 'show'}</span>
							</button>

							{#if t._overridesOpen}
								<div class="grid grid-cols-1 gap-2 rounded bg-muted/20 p-2 sm:grid-cols-3">
									<div class="flex flex-col gap-1">
										<Label class="text-[10px] uppercase text-muted-foreground">
											Timeout (ms)
										</Label>
										<Input
											type="number"
											min="100"
											step="100"
											value={t.timeout_ms ?? ''}
											oninput={(e) => {
												const raw = e.currentTarget.value;
												t.timeout_ms = raw === '' ? null : Number(raw);
											}}
											placeholder="backend default"
										/>
									</div>
									{#if kind === 'chat'}
										<div class="flex flex-col gap-1">
											<Label class="text-[10px] uppercase text-muted-foreground">Max tokens</Label>
											<Input
												type="number"
												min="1"
												value={t.max_tokens ?? ''}
												oninput={(e) => {
													const raw = e.currentTarget.value;
													t.max_tokens = raw === '' ? null : Number(raw);
												}}
												placeholder="(unset)"
											/>
										</div>
										<div class="flex flex-col gap-1">
											<Label class="text-[10px] uppercase text-muted-foreground">Temperature</Label>
											<Input
												type="number"
												min="0"
												max="2"
												step="0.05"
												value={t.temperature ?? ''}
												oninput={(e) => {
													const raw = e.currentTarget.value;
													t.temperature = raw === '' ? null : Number(raw);
												}}
												placeholder="(unset)"
											/>
										</div>
									{:else}
										<div class="col-span-2 flex items-center text-xs text-muted-foreground">
											max_tokens / temperature don't apply to embeddings; only timeout_ms is used.
										</div>
									{/if}
								</div>
							{/if}
						</div>
					{/each}
				</div>

				{#if targetsError}
					<p class="text-xs text-amber-600">{targetsError}</p>
				{/if}
			{/if}
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={close}>Cancel</Button>
			<Button onclick={submit} disabled={!canSubmit || backends.length === 0}>
				{submitting ? 'Saving…' : mode === 'edit' ? 'Save changes' : 'Add mapping'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
