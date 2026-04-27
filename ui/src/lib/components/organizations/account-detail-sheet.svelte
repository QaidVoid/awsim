<script lang="ts">
	import {
		describeAccount,
		accountStatusVariant,
		type Account
	} from '$lib/api/organizations';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { toast } from 'svelte-sonner';

	interface Props {
		account: Account | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { account, open, onOpenChange }: Props = $props();

	let detail = $state<Account | null>(null);
	let loading = $state(false);

	function fmt(ms?: number): string {
		if (!ms) return '—';
		try {
			return new Date(ms * 1000).toLocaleString();
		} catch {
			return String(ms);
		}
	}

	$effect(() => {
		if (!open || !account) {
			detail = null;
			return;
		}
		const id = account.id;
		loading = true;
		describeAccount(id)
			.then((d) => {
				detail = d ?? account;
			})
			.catch((err) => {
				toast.error(err instanceof Error ? err.message : 'Failed to load account');
				detail = account;
			})
			.finally(() => (loading = false));
	});
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full overflow-y-auto sm:max-w-md">
		{#if account}
			<SheetHeader>
				<SheetTitle class="font-mono text-base">{account.name}</SheetTitle>
				<SheetDescription>
					<Badge variant={accountStatusVariant((detail ?? account).status)}>
						{(detail ?? account).status}
					</Badge>
				</SheetDescription>
			</SheetHeader>

			<div class="flex flex-col gap-4 p-4">
				{#if loading && !detail}
					<div class="space-y-2">
						{#each Array(3) as _, i (i)}
							<Skeleton class="h-10 w-full" />
						{/each}
					</div>
				{:else}
					{@const d = detail ?? account}
					<dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 rounded-md border border-border bg-card p-3 text-xs">
						<dt class="text-muted-foreground">ID</dt>
						<dd class="font-mono">{d.id}</dd>
						<dt class="text-muted-foreground">ARN</dt>
						<dd class="break-all font-mono">{d.arn}</dd>
						<dt class="text-muted-foreground">Email</dt>
						<dd class="font-mono">{d.email}</dd>
						{#if d.joinedMethod}
							<dt class="text-muted-foreground">Joined via</dt>
							<dd class="font-mono">{d.joinedMethod}</dd>
						{/if}
						<dt class="text-muted-foreground">Joined</dt>
						<dd class="font-mono">{fmt(d.joinedTimestamp)}</dd>
					</dl>
				{/if}
			</div>
		{/if}
	</SheetContent>
</Sheet>
