<script lang="ts">
	import { Textarea } from '$lib/components/ui/textarea';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import AlertCircle from '@lucide/svelte/icons/alert-circle';

	interface Props {
		value: string;
		onValueChange?: (v: string) => void;
		label?: string;
		id?: string;
		rows?: number;
		readonly?: boolean;
	}

	let {
		value = $bindable(),
		onValueChange,
		label = 'Policy document',
		id = 'policy-doc',
		rows = 14,
		readonly = false
	}: Props = $props();

	let validation = $state<{ ok: boolean; error?: string } | null>(null);

	function validate() {
		try {
			JSON.parse(value);
			validation = { ok: true };
		} catch (e) {
			validation = {
				ok: false,
				error: e instanceof Error ? e.message : 'Invalid JSON'
			};
		}
	}

	function format() {
		try {
			value = JSON.stringify(JSON.parse(value), null, 2);
			onValueChange?.(value);
			validation = { ok: true };
		} catch (e) {
			validation = {
				ok: false,
				error: e instanceof Error ? e.message : 'Invalid JSON'
			};
		}
	}
</script>

<div class="flex flex-col gap-2">
	<div class="flex items-center justify-between">
		<Label for={id} class="text-xs uppercase tracking-wide text-muted-foreground">{label}</Label>
		<div class="flex items-center gap-2">
			<Button type="button" variant="ghost" size="xs" onclick={validate}>Validate</Button>
			{#if !readonly}
				<Button type="button" variant="ghost" size="xs" onclick={format}>Format</Button>
			{/if}
		</div>
	</div>
	<Textarea
		{id}
		bind:value
		oninput={() => onValueChange?.(value)}
		{readonly}
		{rows}
		class="font-mono text-xs"
	/>
	{#if validation}
		<div
			class="flex items-center gap-1.5 text-xs {validation.ok
				? 'text-emerald-500'
				: 'text-destructive'}"
		>
			{#if validation.ok}
				<CheckCircle2 class="size-3.5" />
				<span>Valid JSON</span>
			{:else}
				<AlertCircle class="size-3.5" />
				<span class="truncate">{validation.error}</span>
			{/if}
		</div>
	{/if}
</div>
