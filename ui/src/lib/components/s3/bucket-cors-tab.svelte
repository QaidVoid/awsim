<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		getBucketCors,
		putBucketCors,
		deleteBucketCors,
		type CorsRule
	} from '$lib/api/s3';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Badge } from '$lib/components/ui/badge';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Save from '@lucide/svelte/icons/save';
	import FileText from '@lucide/svelte/icons/file-text';
	import FormInput from '@lucide/svelte/icons/form-input';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';

	interface Props {
		bucket: string;
	}

	let { bucket }: Props = $props();

	let rules = $state<CorsRule[]>([]);
	let loading = $state(true);
	let saving = $state(false);
	let mode = $state<'form' | 'json'>('form');
	let jsonText = $state('');
	let jsonError = $state<string | null>(null);

	onMount(load);

	async function load() {
		loading = true;
		try {
			rules = await getBucketCors(bucket);
			syncJsonFromRules();
		} catch {
			rules = [];
			syncJsonFromRules();
		} finally {
			loading = false;
		}
	}

	function syncJsonFromRules() {
		jsonText = JSON.stringify(rules, null, 2);
		jsonError = null;
	}

	function syncRulesFromJson() {
		try {
			const parsed = JSON.parse(jsonText);
			if (!Array.isArray(parsed)) {
				jsonError = 'JSON must be an array of CORS rules';
				return false;
			}
			for (const r of parsed) {
				if (!Array.isArray(r.AllowedMethods) || !Array.isArray(r.AllowedOrigins)) {
					jsonError = 'Each rule must have AllowedMethods and AllowedOrigins arrays';
					return false;
				}
			}
			rules = parsed as CorsRule[];
			jsonError = null;
			return true;
		} catch (e) {
			jsonError = e instanceof Error ? `Invalid JSON: ${e.message}` : 'Invalid JSON';
			return false;
		}
	}

	function switchMode(m: 'form' | 'json') {
		if (m === 'json') {
			syncJsonFromRules();
		} else {
			syncRulesFromJson();
		}
		mode = m;
	}

	function addRule() {
		rules = [
			...rules,
			{ AllowedMethods: ['GET'], AllowedOrigins: ['*'], AllowedHeaders: ['*'] }
		];
		syncJsonFromRules();
	}

	function removeRule(index: number) {
		rules = rules.filter((_, i) => i !== index);
		syncJsonFromRules();
	}

	function updateMethods(index: number, value: string) {
		const methods = value
			.split(',')
			.map((s) => s.trim().toUpperCase())
			.filter(Boolean);
		rules = rules.map((r, i) => (i === index ? { ...r, AllowedMethods: methods } : r));
		syncJsonFromRules();
	}

	function updateOrigins(index: number, value: string) {
		const origins = value.split(',').map((s) => s.trim()).filter(Boolean);
		rules = rules.map((r, i) => (i === index ? { ...r, AllowedOrigins: origins } : r));
		syncJsonFromRules();
	}

	function updateHeaders(index: number, value: string) {
		const headers = value.split(',').map((s) => s.trim()).filter(Boolean);
		rules = rules.map((r, i) =>
			i === index ? { ...r, AllowedHeaders: headers.length ? headers : undefined } : r
		);
		syncJsonFromRules();
	}

	function updateExposeHeaders(index: number, value: string) {
		const headers = value.split(',').map((s) => s.trim()).filter(Boolean);
		rules = rules.map((r, i) =>
			i === index ? { ...r, ExposeHeaders: headers.length ? headers : undefined } : r
		);
		syncJsonFromRules();
	}

	function updateMaxAge(index: number, value: string) {
		const seconds = parseInt(value, 10);
		rules = rules.map((r, i) =>
			i === index ? { ...r, MaxAgeSeconds: isNaN(seconds) ? undefined : seconds } : r
		);
		syncJsonFromRules();
	}

	async function save() {
		if (mode === 'json' && !syncRulesFromJson()) return;
		saving = true;
		try {
			if (rules.length === 0) {
				await deleteBucketCors(bucket);
			} else {
				await putBucketCors(bucket, rules);
			}
			toast.success('CORS configuration saved');
			syncJsonFromRules();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save CORS');
		} finally {
			saving = false;
		}
	}

	async function clearCors() {
		saving = true;
		try {
			await deleteBucketCors(bucket);
			rules = [];
			syncJsonFromRules();
			toast.success('CORS configuration removed');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to remove CORS');
		} finally {
			saving = false;
		}
	}
</script>

<div class="h-full overflow-auto p-4">
	{#if loading}
		<div class="flex items-center justify-center py-12 text-muted-foreground">
			<Loader2 class="size-4 animate-spin" />
		</div>
	{:else}
		<div class="mx-auto max-w-2xl">
			<div class="mb-4 flex items-center justify-between">
				<div class="flex items-center gap-2">
					<span class="text-sm font-medium">CORS Rules</span>
					<Badge variant="outline">{rules.length}</Badge>
				</div>
				<div class="flex items-center gap-2">
					<div class="flex rounded-md border border-border">
						<button
							class="flex items-center justify-center rounded-l-md px-2 py-1 text-xs transition-colors {mode === 'form' ? 'bg-primary text-primary-foreground' : 'hover:bg-muted'}"
							onclick={() => switchMode('form')}
							aria-label="Form editor"
						>
							<FormInput class="size-3.5" />
						</button>
						<button
							class="flex items-center justify-center rounded-r-md px-2 py-1 text-xs transition-colors {mode === 'json' ? 'bg-primary text-primary-foreground' : 'hover:bg-muted'}"
							onclick={() => switchMode('json')}
							aria-label="JSON editor"
						>
							<FileText class="size-3.5" />
						</button>
					</div>
					{#if mode === 'form'}
						<Button size="sm" variant="outline" onclick={addRule} disabled={saving}>
							<Plus class="size-3.5" />
							Add rule
						</Button>
					{/if}
					{#if rules.length > 0}
						<Button size="sm" variant="destructive" onclick={clearCors} disabled={saving}>
							Remove all
						</Button>
					{/if}
					<Button size="sm" onclick={save} disabled={saving}>
						{#if saving}
							<Loader2 class="size-3.5 animate-spin" />
						{/if}
						<Save class="size-3.5" />
						Save
					</Button>
				</div>
			</div>

			{#if mode === 'form'}
				{#if rules.length === 0}
					<p class="text-sm text-muted-foreground">
						No CORS rules configured. Click "Add rule" to allow cross-origin requests.
					</p>
				{:else}
					<div class="space-y-4">
						{#each rules as rule, i (i)}
							<div class="rounded-lg border border-border p-3">
								<div class="mb-2 flex items-center justify-between">
									<span class="text-xs font-medium text-muted-foreground">Rule {i + 1}</span>
									<Button variant="ghost" size="icon-xs" onclick={() => removeRule(i)}>
										<X class="size-3" />
									</Button>
								</div>

								<div class="space-y-2">
									<div>
										<Label class="text-[11px]">Allowed Origins</Label>
										<Input
											value={rule.AllowedOrigins.join(', ')}
											onchange={(e) => updateOrigins(i, e.currentTarget.value)}
											placeholder="*"
											class="h-8 text-xs"
										/>
									</div>
									<div>
										<Label class="text-[11px]">Allowed Methods</Label>
										<Input
											value={rule.AllowedMethods.join(', ')}
											onchange={(e) => updateMethods(i, e.currentTarget.value)}
											placeholder="GET, PUT, POST, DELETE"
											class="h-8 text-xs"
										/>
									</div>
									<div>
										<Label class="text-[11px]">Allowed Headers</Label>
										<Input
											value={rule.AllowedHeaders?.join(', ') ?? ''}
											onchange={(e) => updateHeaders(i, e.currentTarget.value)}
											placeholder="*"
											class="h-8 text-xs"
										/>
									</div>
									<div>
										<Label class="text-[11px]">Expose Headers</Label>
										<Input
											value={rule.ExposeHeaders?.join(', ') ?? ''}
											onchange={(e) => updateExposeHeaders(i, e.currentTarget.value)}
											placeholder="ETag, x-amz-request-id"
											class="h-8 text-xs"
										/>
									</div>
									<div>
										<Label class="text-[11px]">Max Age (seconds)</Label>
										<Input
											value={rule.MaxAgeSeconds?.toString() ?? ''}
											onchange={(e) => updateMaxAge(i, e.currentTarget.value)}
											placeholder="3600"
											type="number"
											class="h-8 text-xs"
										/>
									</div>
								</div>
							</div>
						{/each}
					</div>
				{/if}
			{:else}
				<Textarea
					bind:value={jsonText}
					rows={20}
					class="font-mono text-xs"
					placeholder="[]"
				/>
				{#if jsonError}
					<p class="mt-2 text-xs text-destructive">{jsonError}</p>
				{/if}
			{/if}
		</div>
	{/if}
</div>
