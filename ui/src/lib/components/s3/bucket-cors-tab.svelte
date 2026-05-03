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
	import { Badge } from '$lib/components/ui/badge';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Save from '@lucide/svelte/icons/save';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';

	interface Props {
		bucket: string;
	}

	let { bucket }: Props = $props();

	let rules = $state<CorsRule[]>([]);
	let loading = $state(true);
	let saving = $state(false);

	onMount(load);

	async function load() {
		loading = true;
		try {
			rules = await getBucketCors(bucket);
		} catch {
			rules = [];
		} finally {
			loading = false;
		}
	}

	function addRule() {
		rules = [
			...rules,
			{ AllowedMethods: ['GET'], AllowedOrigins: ['*'], AllowedHeaders: ['*'] }
		];
	}

	function removeRule(index: number) {
		rules = rules.filter((_, i) => i !== index);
	}

	function updateMethods(index: number, value: string) {
		const methods = value
			.split(',')
			.map((s) => s.trim().toUpperCase())
			.filter(Boolean);
		rules = rules.map((r, i) => (i === index ? { ...r, AllowedMethods: methods } : r));
	}

	function updateOrigins(index: number, value: string) {
		const origins = value.split(',').map((s) => s.trim()).filter(Boolean);
		rules = rules.map((r, i) => (i === index ? { ...r, AllowedOrigins: origins } : r));
	}

	function updateHeaders(index: number, value: string) {
		const headers = value.split(',').map((s) => s.trim()).filter(Boolean);
		rules = rules.map((r, i) =>
			i === index ? { ...r, AllowedHeaders: headers.length ? headers : undefined } : r
		);
	}

	async function save() {
		saving = true;
		try {
			if (rules.length === 0) {
				await deleteBucketCors(bucket);
			} else {
				await putBucketCors(bucket, rules);
			}
			toast.success('CORS configuration saved');
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
				<div class="flex gap-2">
					<Button size="sm" variant="outline" onclick={addRule} disabled={saving}>
						<Plus class="size-3.5" />
						Add rule
					</Button>
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
							</div>
						</div>
					{/each}
				</div>
			{/if}
		</div>
	{/if}
</div>
