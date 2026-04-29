<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Badge } from '$lib/components/ui/badge';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlayIcon from '@lucide/svelte/icons/play';
	import { toast } from 'svelte-sonner';
	import {
		uploadArchive,
		listJobs,
		initiateJob,
		type Vault,
		type ArchiveJob
	} from '$lib/api/glacier';

	interface Props {
		open: boolean;
		vault: Vault | null;
		onOpenChange: (open: boolean) => void;
		onChanged?: () => void;
	}

	let { open, vault, onOpenChange, onChanged }: Props = $props();

	let jobs = $state<ArchiveJob[]>([]);
	let loading = $state(false);
	let archiveContent = $state('Hello from the AWSim glacier UI');
	let archiveDesc = $state('');
	let uploading = $state(false);
	let inventorying = $state(false);

	$effect(() => {
		if (open && vault) {
			void load(vault.vaultName);
		} else if (!open) {
			jobs = [];
		}
	});

	async function load(name: string) {
		loading = true;
		try {
			jobs = await listJobs(name);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load jobs');
		} finally {
			loading = false;
		}
	}

	async function upload() {
		if (!vault) return;
		if (!archiveContent) return toast.error('Archive content cannot be empty.');
		uploading = true;
		try {
			const b64 = btoa(archiveContent);
			const r = await uploadArchive(vault.vaultName, b64, archiveDesc.trim() || undefined);
			toast.success(`Uploaded archive ${r.archiveId.slice(0, 12)}…`);
			archiveContent = '';
			archiveDesc = '';
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to upload');
		} finally {
			uploading = false;
		}
	}

	async function startInventory() {
		if (!vault) return;
		inventorying = true;
		try {
			await initiateJob(vault.vaultName, 'inventory-retrieval');
			toast.success('Inventory job initiated.');
			await load(vault.vaultName);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to initiate job');
		} finally {
			inventorying = false;
		}
	}

	function statusColor(s: string): string {
		if (s === 'Succeeded') return 'text-green-500';
		if (s === 'Failed') return 'text-destructive';
		return 'text-amber-500';
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>Vault details</SheetTitle>
			<SheetDescription>
				{#if vault}
					<span class="font-mono text-xs">{vault.vaultName}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4">
			{#if vault}
				<div class="grid grid-cols-2 gap-3 text-xs">
					<div>
						<div class="font-semibold text-muted-foreground">Archives</div>
						<div>{vault.numberOfArchives}</div>
					</div>
					<div>
						<div class="font-semibold text-muted-foreground">Size</div>
						<div>{vault.sizeInBytes} bytes</div>
					</div>
				</div>

				<div class="space-y-2 rounded-md border border-border p-3">
					<div class="text-xs font-semibold">Upload archive</div>
					<Input
						bind:value={archiveDesc}
						placeholder="description (optional)"
						class="h-8 text-xs"
					/>
					<Textarea bind:value={archiveContent} rows={4} class="font-mono text-xs" />
					<Button size="sm" onclick={upload} disabled={uploading}>
						{uploading ? 'Uploading…' : 'Upload'}
					</Button>
					<p class="text-[11px] text-muted-foreground">
						Content is base64-encoded client-side and uploaded as a single shot.
					</p>
				</div>

				<div class="space-y-2">
					<div class="flex items-center justify-between">
						<div class="text-xs font-semibold text-muted-foreground">
							Jobs ({jobs.length})
						</div>
						<div class="flex items-center gap-1">
							<Button
								variant="ghost"
								size="xs"
								onclick={() => vault && load(vault.vaultName)}
								disabled={loading}
							>
								<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
							</Button>
							<Button size="xs" variant="outline" onclick={startInventory} disabled={inventorying}>
								<PlayIcon />
								Inventory
							</Button>
						</div>
					</div>
					{#if jobs.length === 0}
						<div class="rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground">
							No jobs.
						</div>
					{:else}
						<div class="space-y-1.5">
							{#each jobs as j (j.jobId)}
								<div class="flex items-start justify-between rounded-md border border-border p-2 text-xs">
									<div class="flex flex-col gap-0.5">
										<span class="font-mono">{j.jobId.slice(0, 24)}…</span>
										<span class="text-muted-foreground">{j.action}</span>
										{#if j.archiveId}
											<span class="font-mono text-[10px] text-muted-foreground">
												archive: {j.archiveId.slice(0, 16)}…
											</span>
										{/if}
									</div>
									<Badge variant="outline" class={`h-5 px-2 text-[10px] ${statusColor(j.statusCode)}`}>
										{j.statusCode}
									</Badge>
								</div>
							{/each}
						</div>
					{/if}
				</div>
			{/if}
		</div>
	</SheetContent>
</Sheet>
