<script lang="ts">
	import { onMount } from 'svelte';
	import { listRoles, type IamRole } from '$lib/api/iam';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import {
		Popover,
		PopoverContent,
		PopoverTrigger
	} from '$lib/components/ui/popover';
	import ChevronsUpDown from '@lucide/svelte/icons/chevrons-up-down';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		/**
		 * Current ARN value. Bind from the parent so a free-typed ARN
		 * survives even if the user closes the picker without choosing.
		 */
		value: string;
		/** Forwarded to the underlying Input for accessibility. */
		id?: string;
		placeholder?: string;
		disabled?: boolean;
	}

	let {
		value = $bindable(''),
		id,
		placeholder = 'arn:aws:iam::000000000000:role/MyRole',
		disabled = false
	}: Props = $props();

	let roles = $state<IamRole[]>([]);
	let loading = $state(false);
	let loaded = $state(false);
	let loadError = $state<string | null>(null);
	let open = $state(false);
	let filter = $state('');

	const candidates = $derived(
		roles.filter((r) =>
			filter.trim()
				? r.roleName.toLowerCase().includes(filter.trim().toLowerCase()) ||
					r.arn.toLowerCase().includes(filter.trim().toLowerCase())
				: true
		)
	);

	onMount(load);

	async function load() {
		if (loading || loaded) return;
		loading = true;
		try {
			roles = await listRoles();
			loaded = true;
		} catch (e) {
			// Don't block typing — IAM may be unreachable, the user can
			// still paste an ARN by hand.
			loadError = e instanceof Error ? e.message : 'Failed to load IAM roles';
		} finally {
			loading = false;
		}
	}

	function pick(arn: string) {
		value = arn;
		open = false;
		filter = '';
	}
</script>

<div class="flex items-stretch gap-1.5">
	<Input
		{id}
		bind:value
		{placeholder}
		{disabled}
		class="font-mono text-xs"
		autocomplete="off"
	/>
	<Popover bind:open>
		<PopoverTrigger>
			<Button
				type="button"
				variant="outline"
				size="icon-sm"
				{disabled}
				title="Pick from existing IAM roles"
				onclick={load}
			>
				<ChevronsUpDown class="size-3.5" />
			</Button>
		</PopoverTrigger>
		<PopoverContent class="w-80 p-2">
			<Input
				placeholder="Filter roles..."
				bind:value={filter}
				class="mb-2 h-7 text-xs"
				autocomplete="off"
			/>
			{#if loading}
				<p class="px-2 py-1.5 text-xs text-muted-foreground">
					<Loader2 class="inline size-3 animate-spin" /> Loading roles...
				</p>
			{:else if loadError}
				<p class="px-2 py-1.5 text-xs text-destructive">{loadError}</p>
			{:else if candidates.length === 0}
				<p class="px-2 py-1.5 text-xs text-muted-foreground">
					{roles.length === 0 ? 'No IAM roles in this account.' : 'No matching roles.'}
				</p>
			{:else}
				<ul class="max-h-64 space-y-0.5 overflow-y-auto">
					{#each candidates as r (r.arn)}
						<li>
							<button
								type="button"
								class="w-full rounded px-2 py-1.5 text-left hover:bg-muted"
								onclick={() => pick(r.arn)}
							>
								<div class="text-sm font-medium">{r.roleName}</div>
								<div class="truncate font-mono text-[11px] text-muted-foreground">
									{r.arn}
								</div>
							</button>
						</li>
					{/each}
				</ul>
			{/if}
		</PopoverContent>
	</Popover>
</div>
