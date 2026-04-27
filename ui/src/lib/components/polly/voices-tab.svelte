<script lang="ts">
	import { onMount } from 'svelte';
	import { describeVoices, type Voice } from '$lib/api/polly';
	import { DataTable, EmptyState } from '$lib/components/service';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import SearchIcon from '@lucide/svelte/icons/search';
	import Volume2Icon from '@lucide/svelte/icons/volume-2';
	import { toast } from 'svelte-sonner';

	let voices = $state<Voice[]>([]);
	let loading = $state(true);
	let filter = $state('');
	let language = $state<string>('all');
	let gender = $state<string>('all');

	let languages = $derived(
		Array.from(new Set(voices.map((v) => v.languageCode)))
			.filter(Boolean)
			.sort()
	);
	let filtered = $derived(
		voices.filter(
			(v) =>
				(language === 'all' || v.languageCode === language) &&
				(gender === 'all' || v.gender === gender) &&
				(filter.trim() === '' ||
					v.name.toLowerCase().includes(filter.trim().toLowerCase()) ||
					v.languageName.toLowerCase().includes(filter.trim().toLowerCase()))
		)
	);

	onMount(load);

	async function load() {
		loading = true;
		try {
			voices = await describeVoices();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load voices');
		} finally {
			loading = false;
		}
	}

	function genderVariant(g: string): 'secondary' | 'outline' {
		if (g === 'Female' || g === 'Male') return 'secondary';
		return 'outline';
	}
</script>

{#snippet languageCell(row: Voice)}
	<span class="text-xs">
		{row.languageName}
		<span class="text-muted-foreground">({row.languageCode})</span>
	</span>
{/snippet}

{#snippet genderCell(row: Voice)}
	<Badge variant={genderVariant(row.gender)} class="h-4 px-1 text-[10px]">
		{row.gender || '—'}
	</Badge>
{/snippet}

{#snippet enginesCell(row: Voice)}
	<div class="flex flex-wrap gap-1">
		{#each row.supportedEngines as e (e)}
			<Badge variant="outline" class="h-4 px-1 text-[10px]">{e}</Badge>
		{:else}
			<span class="text-[10px] text-muted-foreground">—</span>
		{/each}
	</div>
{/snippet}

<div class="flex flex-col gap-3 p-4">
	<div class="flex flex-wrap items-center gap-2">
		<div class="relative min-w-48 flex-1">
			<SearchIcon
				class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground"
			/>
			<Input
				type="search"
				placeholder="Filter by name or language"
				bind:value={filter}
				class="h-8 pl-7 text-xs"
			/>
		</div>
		<select
			bind:value={language}
			aria-label="Filter by language"
			class="h-8 rounded-md border border-input bg-background px-2 text-xs"
		>
			<option value="all">All languages</option>
			{#each languages as l (l)}
				<option value={l}>{l}</option>
			{/each}
		</select>
		<select
			bind:value={gender}
			aria-label="Filter by gender"
			class="h-8 rounded-md border border-input bg-background px-2 text-xs"
		>
			<option value="all">All genders</option>
			<option value="Female">Female</option>
			<option value="Male">Male</option>
		</select>
		<Button variant="outline" size="sm" onclick={load} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
	</div>

	<DataTable
		rows={filtered}
		{loading}
		columns={[
			{ key: 'name', label: 'Name' },
			{ key: 'language', label: 'Language', cell: languageCell },
			{ key: 'gender', label: 'Gender', cell: genderCell },
			{ key: 'engines', label: 'Engines', cell: enginesCell },
		]}
		rowKey={(r) => `${r.id}-${r.languageCode}`}
	>
		{#snippet empty()}
			<EmptyState
				icon={Volume2Icon}
				title="No voices"
				description="No voices match the current filter."
			/>
		{/snippet}
	</DataTable>
</div>
