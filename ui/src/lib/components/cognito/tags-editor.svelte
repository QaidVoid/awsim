<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import {
		describeUserPool,
		listTagsForResource,
		tagResource,
		untagResource,
		type TagMap
	} from '$lib/api/cognito';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';

	interface Props {
		poolId: string;
	}

	let { poolId }: Props = $props();

	let arn = $state<string | undefined>(undefined);
	let tags = $state<{ key: string; value: string }[]>([]);
	let loading = $state(true);
	let saving = $state(false);
	let newKey = $state('');
	let newValue = $state('');

	onMount(load);

	async function load() {
		loading = true;
		try {
			const detail = await describeUserPool(poolId);
			arn = detail.arn;
			if (!arn) {
				tags = [];
				return;
			}
			const map = await listTagsForResource(arn);
			tags = Object.entries(map).map(([key, value]) => ({ key, value }));
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load tags');
		} finally {
			loading = false;
		}
	}

	async function addTag() {
		const k = newKey.trim();
		const v = newValue.trim();
		if (!k) {
			toast.error('Tag key is required');
			return;
		}
		if (!arn) return;
		if (tags.some((t) => t.key === k)) {
			toast.error(`Tag "${k}" already exists`);
			return;
		}
		saving = true;
		try {
			await tagResource(arn, { [k]: v });
			toast.success(`Added tag ${k}`);
			newKey = '';
			newValue = '';
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Add failed');
		} finally {
			saving = false;
		}
	}

	async function removeTag(key: string) {
		if (!arn) return;
		try {
			await untagResource(arn, [key]);
			toast.success(`Removed tag ${key}`);
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Remove failed');
		}
	}

	async function updateValue(key: string, value: string) {
		if (!arn) return;
		try {
			await tagResource(arn, { [key]: value });
			toast.success(`Updated ${key}`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Update failed');
		}
	}
</script>

<div class="space-y-3 rounded border border-border/60 px-3 py-3">
	<div>
		<h3 class="text-sm font-semibold">Tags</h3>
		<p class="text-xs text-muted-foreground">
			Resource tags propagate to billing reports and IAM condition keys.
		</p>
	</div>

	{#if loading}
		<p class="text-xs text-muted-foreground">
			<Loader2 class="inline size-3 animate-spin" /> Loading...
		</p>
	{:else if !arn}
		<p class="text-xs text-muted-foreground">Pool ARN unavailable — tags can't be edited.</p>
	{:else}
		{#if tags.length === 0}
			<p class="text-xs text-muted-foreground">No tags. Add one below.</p>
		{:else}
			<ul class="space-y-1.5">
				{#each tags as t (t.key)}
					<li class="grid grid-cols-[10rem_minmax(0,1fr)_auto] items-center gap-2 text-sm">
						<Badge variant="outline" class="font-mono text-xs">{t.key}</Badge>
						<Input
							bind:value={t.value}
							onblur={() => updateValue(t.key, t.value)}
							class="h-7 min-w-0 text-xs"
						/>
						<Button
							variant="ghost"
							size="icon-sm"
							onclick={() => removeTag(t.key)}
							class="text-destructive hover:text-destructive"
							title="Remove tag"
						>
							<X class="size-3.5" />
						</Button>
					</li>
				{/each}
			</ul>
		{/if}
		<div class="grid grid-cols-[10rem_minmax(0,1fr)_auto] items-center gap-2 pt-1">
			<Input bind:value={newKey} placeholder="key" class="h-7 font-mono text-xs" />
			<Input bind:value={newValue} placeholder="value" class="h-7 min-w-0 text-xs" />
			<Button size="xs" onclick={addTag} disabled={saving || !newKey.trim()}>
				<Plus class="size-3" />
			</Button>
		</div>
	{/if}
</div>
