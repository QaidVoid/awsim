<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import { Badge } from '$lib/components/ui/badge';
	import { ListSkeleton } from '$lib/components/service';
	import { toast } from 'svelte-sonner';
	import {
		describePermissionSet,
		listManagedPoliciesInPermissionSet,
		getInlinePolicyForPermissionSet,
		type PermissionSet,
		type ManagedPolicy
	} from '$lib/api/sso-admin';

	interface Props {
		instanceArn: string;
		permissionSetArn: string | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { instanceArn, permissionSetArn, open = $bindable(), onOpenChange }: Props = $props();

	let active = $state('overview');
	let loading = $state(false);
	let detail = $state<PermissionSet | null>(null);
	let managed = $state<ManagedPolicy[]>([]);
	let inline = $state<string | null>(null);

	$effect(() => {
		if (open && permissionSetArn) load(permissionSetArn);
	});

	async function load(arn: string) {
		detail = null;
		managed = [];
		inline = null;
		loading = true;
		try {
			const [d, m, ip] = await Promise.all([
				describePermissionSet(instanceArn, arn),
				listManagedPoliciesInPermissionSet(instanceArn, arn).catch(() => []),
				getInlinePolicyForPermissionSet(instanceArn, arn).catch(() => null)
			]);
			detail = d;
			managed = m;
			inline = ip?.policyDocument ?? null;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load permission set');
		} finally {
			loading = false;
		}
	}
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-2xl overflow-y-auto sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>{detail?.name ?? 'Permission set'}</SheetTitle>
			<SheetDescription class="truncate font-mono text-xs">
				{permissionSetArn ?? ''}
			</SheetDescription>
		</SheetHeader>

		<div class="px-6 pb-6">
			{#if loading}
				<ListSkeleton rows={4} />
			{:else}
				<Tabs bind:value={active} class="mt-2">
					<TabsList variant="line">
						<TabsTrigger value="overview">Overview</TabsTrigger>
						<TabsTrigger value="managed">Managed policies ({managed.length})</TabsTrigger>
						<TabsTrigger value="inline">Inline policy</TabsTrigger>
					</TabsList>

					<TabsContent value="overview" class="mt-4">
						<dl class="grid grid-cols-3 gap-x-4 gap-y-2 text-sm">
							<dt class="text-muted-foreground">Name</dt>
							<dd class="col-span-2 font-mono text-xs">{detail?.name ?? '—'}</dd>
							<dt class="text-muted-foreground">Description</dt>
							<dd class="col-span-2 text-xs">{detail?.description ?? '—'}</dd>
							<dt class="text-muted-foreground">Session duration</dt>
							<dd class="col-span-2 font-mono text-xs">{detail?.sessionDuration ?? '—'}</dd>
							<dt class="text-muted-foreground">Relay state</dt>
							<dd class="col-span-2 break-all font-mono text-xs">{detail?.relayState ?? '—'}</dd>
							<dt class="text-muted-foreground">Created</dt>
							<dd class="col-span-2 text-xs">{detail?.createdDate ?? '—'}</dd>
						</dl>
					</TabsContent>

					<TabsContent value="managed" class="mt-4">
						{#if managed.length === 0}
							<p class="text-xs text-muted-foreground">
								No managed policies attached to this permission set.
							</p>
						{:else}
							<ul class="flex flex-col gap-2">
								{#each managed as p (p.arn)}
									<li
										class="flex items-center justify-between rounded-md border border-border bg-card/40 px-3 py-2"
									>
										<div class="min-w-0">
											<div class="font-mono text-xs">{p.name ?? '—'}</div>
											<div class="truncate font-mono text-[10px] text-muted-foreground">
												{p.arn}
											</div>
										</div>
										<Badge variant="outline" class="h-4 px-1.5 text-[10px]">managed</Badge>
									</li>
								{/each}
							</ul>
						{/if}
					</TabsContent>

					<TabsContent value="inline" class="mt-4">
						{#if inline === null}
							<p class="text-xs text-muted-foreground">No inline policy attached.</p>
						{:else}
							<pre
								class="max-h-[60vh] overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-[11px] whitespace-pre-wrap break-all">{inline}</pre>
						{/if}
					</TabsContent>
				</Tabs>
			{/if}
		</div>
	</SheetContent>
</Sheet>
