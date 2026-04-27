<script lang="ts">
	import { describePolicy, type Policy } from '$lib/api/organizations';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import Copy from '@lucide/svelte/icons/copy';
	import { toast } from 'svelte-sonner';

	interface Props {
		policy: Policy | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { policy, open, onOpenChange }: Props = $props();

	let content = $state('');
	let loading = $state(false);

	function pretty(raw: string): string {
		const t = raw.trim();
		if (!t) return '';
		try {
			return JSON.stringify(JSON.parse(t), null, 2);
		} catch {
			return t;
		}
	}

	$effect(() => {
		if (!open || !policy) {
			content = '';
			return;
		}
		const id = policy.id;
		loading = true;
		describePolicy(id)
			.then((d) => {
				content = pretty(d?.content ?? '');
			})
			.catch((err) =>
				toast.error(err instanceof Error ? err.message : 'Failed to load policy')
			)
			.finally(() => (loading = false));
	});

	async function copy() {
		try {
			await navigator.clipboard.writeText(content);
			toast.success('Policy copied');
		} catch {
			toast.error('Copy failed');
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full overflow-y-auto sm:max-w-2xl">
		{#if policy}
			<SheetHeader>
				<SheetTitle class="font-mono text-base">{policy.name}</SheetTitle>
				<SheetDescription>
					<Badge variant="outline" class="font-mono text-[10px]">{policy.type}</Badge>
					{#if policy.awsManaged}
						<Badge variant="secondary" class="ml-1 text-[10px]">AWS managed</Badge>
					{/if}
				</SheetDescription>
			</SheetHeader>

			<div class="flex flex-col gap-4 p-4">
				<div class="rounded-md border border-border bg-card p-3 text-xs">
					<div class="text-muted-foreground">ID</div>
					<div class="mt-0.5 break-all font-mono">{policy.id}</div>
					{#if policy.description}
						<div class="mt-2 text-muted-foreground">Description</div>
						<div class="mt-0.5">{policy.description}</div>
					{/if}
				</div>

				<div class="flex flex-col gap-1.5">
					<div class="flex items-center justify-between">
						<Label for="org-scp-content">Document</Label>
						<Button
							type="button"
							variant="ghost"
							size="sm"
							onclick={copy}
							disabled={!content || loading}
						>
							<Copy />
							Copy
						</Button>
					</div>
					{#if loading}
						<Skeleton class="h-64 w-full" />
					{:else}
						<Textarea
							id="org-scp-content"
							value={content}
							readonly
							rows={20}
							class="font-mono text-xs"
						/>
					{/if}
				</div>
			</div>
		{/if}
	</SheetContent>
</Sheet>
