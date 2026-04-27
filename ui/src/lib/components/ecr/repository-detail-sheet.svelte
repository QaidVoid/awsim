<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle,
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { toast } from 'svelte-sonner';
	import type { Repository } from '$lib/api/ecr';

	interface Props {
		repo: Repository | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { repo, open = $bindable(), onOpenChange }: Props = $props();

	function formatDate(iso: string): string {
		if (!iso) return '—';
		try {
			return new Date(iso).toLocaleString();
		} catch {
			return iso;
		}
	}

	async function copy(value: string, label: string) {
		try {
			await navigator.clipboard.writeText(value);
			toast.success(`${label} copied.`);
		} catch {
			toast.error('Copy failed.');
		}
	}

	const pushSnippet = $derived(
		repo
			? [
					`# Build & tag`,
					`docker build -t ${repo.repositoryName}:latest .`,
					`docker tag ${repo.repositoryName}:latest ${repo.repositoryUri}:latest`,
					``,
					`# Push`,
					`docker push ${repo.repositoryUri}:latest`,
				].join('\n')
			: ''
	);
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-2xl overflow-y-auto sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>{repo?.repositoryName ?? ''}</SheetTitle>
			<SheetDescription class="truncate font-mono text-xs">
				{repo?.repositoryArn ?? ''}
			</SheetDescription>
		</SheetHeader>

		{#if repo}
			<div class="flex flex-col gap-4 px-6 pb-6">
				<section class="rounded-md border border-border bg-card/40 p-4">
					<h3 class="mb-3 text-sm font-semibold">Identity</h3>
					<dl class="grid grid-cols-[140px_1fr] gap-x-4 gap-y-2 text-xs">
						<dt class="text-muted-foreground">Tag mutability</dt>
						<dd>
							<Badge variant="outline" class="h-4 px-1.5 text-[10px]">
								{repo.imageTagMutability}
							</Badge>
						</dd>
						<dt class="text-muted-foreground">Scan on push</dt>
						<dd>
							{#if repo.scanOnPush}
								<Badge variant="outline" class="h-4 px-1.5 text-[10px] text-green-500"
									>enabled</Badge
								>
							{:else}
								<Badge variant="outline" class="h-4 px-1.5 text-[10px]">disabled</Badge>
							{/if}
						</dd>
						<dt class="text-muted-foreground">Registry</dt>
						<dd class="font-mono text-[11px]">{repo.registryId || '—'}</dd>
						<dt class="text-muted-foreground">Created</dt>
						<dd>{formatDate(repo.createdAt)}</dd>
					</dl>
				</section>

				<section class="rounded-md border border-border bg-card/40 p-4">
					<div class="mb-2 flex items-center justify-between">
						<h3 class="text-sm font-semibold">Repository URI</h3>
						<Button
							variant="ghost"
							size="xs"
							onclick={() => copy(repo.repositoryUri, 'URI')}
						>
							<CopyIcon />
							Copy
						</Button>
					</div>
					<pre
						class="overflow-auto rounded-md border border-border bg-muted/40 p-3 text-[11px] font-mono break-all whitespace-pre-wrap">{repo.repositoryUri}</pre>
				</section>

				<section class="rounded-md border border-border bg-card/40 p-4">
					<div class="mb-2 flex items-center justify-between">
						<h3 class="text-sm font-semibold">Push commands</h3>
						<Button variant="ghost" size="xs" onclick={() => copy(pushSnippet, 'Snippet')}>
							<CopyIcon />
							Copy
						</Button>
					</div>
					<pre
						class="overflow-auto rounded-md border border-border bg-muted/40 p-3 text-[11px] font-mono whitespace-pre-wrap">{pushSnippet}</pre>
				</section>
			</div>
		{/if}
	</SheetContent>
</Sheet>
