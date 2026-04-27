<script lang="ts">
	import {
		describeLocationS3,
		type Location,
		type LocationS3Detail
	} from '$lib/api/datasync';
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { toast } from 'svelte-sonner';

	interface Props {
		location: Location | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { location, open, onOpenChange }: Props = $props();

	let detail = $state<LocationS3Detail | null>(null);
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
		if (!open || !location) {
			detail = null;
			return;
		}
		if (location.type !== 'S3') {
			detail = null;
			return;
		}
		const arn = location.locationArn;
		loading = true;
		describeLocationS3(arn)
			.then((d) => {
				detail = d;
			})
			.catch((err) => toast.error(err instanceof Error ? err.message : 'Failed to load detail'))
			.finally(() => (loading = false));
	});
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full overflow-y-auto sm:max-w-lg">
		{#if location}
			<SheetHeader>
				<SheetTitle class="font-mono text-base">{location.locationUri}</SheetTitle>
				<SheetDescription>{location.type ?? 'Location'} location</SheetDescription>
			</SheetHeader>

			<div class="flex flex-col gap-4 p-4">
				<div class="rounded-md border border-border bg-card p-3">
					<div class="text-xs text-muted-foreground">ARN</div>
					<div class="mt-0.5 break-all font-mono text-[11px]">{location.locationArn}</div>
				</div>

				{#if location.type === 'S3'}
					{#if loading}
						<div class="space-y-2">
							{#each Array(3) as _, i (i)}
								<Skeleton class="h-10 w-full" />
							{/each}
						</div>
					{:else if detail}
						<dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 rounded-md border border-border bg-card p-3 text-xs">
							<dt class="text-muted-foreground">Bucket ARN</dt>
							<dd class="break-all font-mono">{detail.s3BucketArn ?? '—'}</dd>
							<dt class="text-muted-foreground">Subdirectory</dt>
							<dd class="font-mono">{detail.subdirectory ?? '—'}</dd>
							<dt class="text-muted-foreground">Storage class</dt>
							<dd class="font-mono">{detail.s3StorageClass ?? '—'}</dd>
							<dt class="text-muted-foreground">Access role</dt>
							<dd class="break-all font-mono">{detail.bucketAccessRoleArn ?? '—'}</dd>
							<dt class="text-muted-foreground">Created</dt>
							<dd class="font-mono">{fmt(detail.creationTime)}</dd>
						</dl>
					{:else}
						<p class="text-xs text-muted-foreground">No detail available.</p>
					{/if}
				{/if}
			</div>
		{/if}
	</SheetContent>
</Sheet>
