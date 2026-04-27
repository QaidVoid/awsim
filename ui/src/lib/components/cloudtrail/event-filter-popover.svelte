<script lang="ts">
	/**
	 * Lookup-attribute filter popover for the CloudTrail Event History tab.
	 */
	import {
		Popover,
		PopoverContent,
		PopoverTrigger,
	} from '$lib/components/ui/popover';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import Filter from '@lucide/svelte/icons/filter';
	import type { LookupAttributeKey } from '$lib/api/cloudtrail';

	interface Props {
		attribute: { key: LookupAttributeKey; value: string } | null;
		onApply: (a: { key: LookupAttributeKey; value: string } | null) => void;
	}

	let { attribute, onApply }: Props = $props();

	let key = $state<LookupAttributeKey>('EventName');
	let value = $state('');
	let open = $state(false);

	// Sync local form state from the parent attribute when the popover
	// opens, so the form reflects the currently active filter.
	$effect(() => {
		if (open) {
			key = attribute?.key ?? 'EventName';
			value = attribute?.value ?? '';
		}
	});

	const KEYS: LookupAttributeKey[] = [
		'EventName',
		'EventSource',
		'EventId',
		'Username',
		'ResourceType',
		'ResourceName',
		'AccessKeyId',
		'ReadOnly',
	];

	function apply() {
		const v = value.trim();
		onApply(v ? { key, value: v } : null);
		open = false;
	}

	function clear() {
		value = '';
		onApply(null);
		open = false;
	}
</script>

<Popover bind:open>
	<PopoverTrigger>
		{#snippet child({ props })}
			<Button {...props} size="sm" variant="outline" class="h-8 gap-1.5 px-2 text-xs">
				<Filter class="size-3.5" />
				{attribute ? `${attribute.key}=${attribute.value}` : 'Filter'}
			</Button>
		{/snippet}
	</PopoverTrigger>
	<PopoverContent class="w-72 space-y-2 p-3">
		<div class="space-y-1.5">
			<Label for="ct-filter-key" class="text-xs">Attribute</Label>
			<select
				id="ct-filter-key"
				bind:value={key}
				class="h-8 w-full rounded-md border border-input bg-transparent px-2 text-xs"
			>
				{#each KEYS as k (k)}
					<option value={k}>{k}</option>
				{/each}
			</select>
		</div>
		<div class="space-y-1.5">
			<Label for="ct-filter-value" class="text-xs">Value</Label>
			<Input id="ct-filter-value" bind:value={value} class="h-8 text-xs" />
		</div>
		<div class="flex items-center justify-end gap-2 pt-1">
			<Button size="sm" variant="ghost" class="h-7 text-xs" onclick={clear}>Clear</Button>
			<Button size="sm" class="h-7 text-xs" onclick={apply}>Apply</Button>
		</div>
	</PopoverContent>
</Popover>
