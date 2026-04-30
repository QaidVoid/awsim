<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { describeUserPool, updateUserPool, type PasswordPolicy } from '$lib/api/cognito';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		poolId: string;
	}

	let { poolId }: Props = $props();

	let original = $state<PasswordPolicy | null>(null);
	let policy = $state<PasswordPolicy>({});
	let minLengthText = $state('8');
	let tempDaysText = $state('7');
	let loading = $state(true);
	let saving = $state(false);

	const dirty = $derived.by(() => {
		if (!original) return false;
		const minLen = Number(minLengthText.trim());
		const tempDays = Number(tempDaysText.trim());
		return (
			minLen !== (original.minimumLength ?? 8) ||
			policy.requireUppercase !== (original.requireUppercase ?? false) ||
			policy.requireLowercase !== (original.requireLowercase ?? false) ||
			policy.requireNumbers !== (original.requireNumbers ?? false) ||
			policy.requireSymbols !== (original.requireSymbols ?? false) ||
			tempDays !== (original.temporaryPasswordValidityDays ?? 7)
		);
	});

	onMount(load);

	async function load() {
		loading = true;
		try {
			const detail = await describeUserPool(poolId);
			original = detail.passwordPolicy ?? {};
			policy = { ...original };
			minLengthText = String(original.minimumLength ?? 8);
			tempDaysText = String(original.temporaryPasswordValidityDays ?? 7);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load policy');
		} finally {
			loading = false;
		}
	}

	async function save() {
		const minLen = Number(minLengthText.trim());
		const tempDays = Number(tempDaysText.trim());
		if (Number.isNaN(minLen) || minLen < 6 || minLen > 99) {
			toast.error('Minimum length must be between 6 and 99');
			return;
		}
		if (Number.isNaN(tempDays) || tempDays < 0 || tempDays > 365) {
			toast.error('Temporary password validity must be 0-365 days');
			return;
		}
		saving = true;
		try {
			await updateUserPool(poolId, {
				passwordPolicy: {
					minimumLength: minLen,
					requireUppercase: policy.requireUppercase ?? false,
					requireLowercase: policy.requireLowercase ?? false,
					requireNumbers: policy.requireNumbers ?? false,
					requireSymbols: policy.requireSymbols ?? false,
					temporaryPasswordValidityDays: tempDays
				}
			});
			toast.success('Password policy saved');
			await load();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Save failed');
		} finally {
			saving = false;
		}
	}

	function reset() {
		if (!original) return;
		policy = { ...original };
		minLengthText = String(original.minimumLength ?? 8);
		tempDaysText = String(original.temporaryPasswordValidityDays ?? 7);
	}
</script>

<div class="space-y-4 rounded border border-border/60 px-3 py-3">
	<div>
		<h3 class="text-sm font-semibold">Password policy</h3>
		<p class="text-xs text-muted-foreground">
			Applies to all users in this pool. Tightening rules doesn't retroactively reset existing
			passwords.
		</p>
	</div>

	{#if loading}
		<p class="text-xs text-muted-foreground">
			<Loader2 class="inline size-3 animate-spin" /> Loading...
		</p>
	{:else}
		<div class="grid gap-3 sm:grid-cols-2">
			<div class="space-y-1.5">
				<Label for="pp-min">Minimum length</Label>
				<Input
					id="pp-min"
					type="number"
					min="6"
					max="99"
					bind:value={minLengthText}
					class="h-8"
				/>
			</div>
			<div class="space-y-1.5">
				<Label for="pp-temp">Temporary password validity (days)</Label>
				<Input
					id="pp-temp"
					type="number"
					min="0"
					max="365"
					bind:value={tempDaysText}
					class="h-8"
				/>
			</div>
		</div>

		<div class="space-y-2">
			<Label class="text-xs uppercase tracking-wide text-muted-foreground">
				Character requirements
			</Label>
			<div class="grid gap-2 sm:grid-cols-2">
				<label class="flex items-center gap-2 text-sm">
					<input type="checkbox" bind:checked={policy.requireUppercase} class="size-3.5" />
					Require uppercase letter
				</label>
				<label class="flex items-center gap-2 text-sm">
					<input type="checkbox" bind:checked={policy.requireLowercase} class="size-3.5" />
					Require lowercase letter
				</label>
				<label class="flex items-center gap-2 text-sm">
					<input type="checkbox" bind:checked={policy.requireNumbers} class="size-3.5" />
					Require number
				</label>
				<label class="flex items-center gap-2 text-sm">
					<input type="checkbox" bind:checked={policy.requireSymbols} class="size-3.5" />
					Require symbol
				</label>
			</div>
		</div>

		<div class="flex justify-end gap-2">
			<Button variant="ghost" size="sm" onclick={reset} disabled={saving || !dirty}>
				Discard
			</Button>
			<Button size="sm" onclick={save} disabled={saving || !dirty}>
				{#if saving}<Loader2 class="size-3.5 animate-spin" />{/if}
				Save
			</Button>
		</div>
	{/if}
</div>
