<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle,
	} from '$lib/components/ui/sheet';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { toast } from 'svelte-sonner';
	import {
		addTags,
		describeListeners,
		describeTags,
		removeTags,
		type Listener,
		type LoadBalancer,
		type Tag,
	} from '$lib/api/elb';

	interface Props {
		lb: LoadBalancer | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onDelete?: (arn: string) => void;
	}

	let { lb, open = $bindable(), onOpenChange, onDelete }: Props = $props();

	let active = $state('overview');
	let listeners = $state<Listener[]>([]);
	let tags = $state<Tag[]>([]);
	let loading = $state(false);
	let newKey = $state('');
	let newValue = $state('');
	let saving = $state(false);

	$effect(() => {
		if (lb && open) void load(lb);
	});

	async function load(target: LoadBalancer) {
		loading = true;
		try {
			const [l, t] = await Promise.all([
				describeListeners(target.arn).catch(() => []),
				describeTags(target.arn).catch(() => []),
			]);
			listeners = l;
			tags = t;
		} finally {
			loading = false;
		}
	}

	async function addTag() {
		if (!lb || !newKey.trim()) return;
		saving = true;
		try {
			await addTags(lb.arn, [{ key: newKey.trim(), value: newValue.trim() }]);
			newKey = '';
			newValue = '';
			tags = await describeTags(lb.arn);
			toast.success('Tag added.');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to add tag');
		} finally {
			saving = false;
		}
	}

	async function deleteTag(key: string) {
		if (!lb) return;
		try {
			await removeTags(lb.arn, [key]);
			tags = tags.filter((t) => t.key !== key);
			toast.success('Tag removed.');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to remove tag');
		}
	}
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-2xl overflow-y-auto sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>{lb?.name ?? ''}</SheetTitle>
			<SheetDescription class="truncate font-mono text-xs">
				{lb?.arn ?? ''}
			</SheetDescription>
		</SheetHeader>

		{#if lb}
			<div class="px-6 pb-6">
				<Tabs bind:value={active} class="mt-2">
					<TabsList variant="line">
						<TabsTrigger value="overview">Overview</TabsTrigger>
						<TabsTrigger value="listeners">Listeners ({listeners.length})</TabsTrigger>
						<TabsTrigger value="tags">Tags ({tags.length})</TabsTrigger>
					</TabsList>

					<TabsContent value="overview" class="mt-4">
						<dl class="grid grid-cols-[140px_1fr] gap-x-4 gap-y-2 text-xs">
							<dt class="text-muted-foreground">Type</dt>
							<dd>
								<Badge variant="outline" class="h-4 px-1.5 text-[10px] uppercase">
									{lb.type}
								</Badge>
							</dd>
							<dt class="text-muted-foreground">Scheme</dt>
							<dd>{lb.scheme}</dd>
							<dt class="text-muted-foreground">VPC</dt>
							<dd class="font-mono text-[11px]">{lb.vpcId || '—'}</dd>
							<dt class="text-muted-foreground">DNS</dt>
							<dd class="font-mono text-[11px] break-all">{lb.dnsName || '—'}</dd>
							<dt class="text-muted-foreground">Created</dt>
							<dd>{lb.createdTime || '—'}</dd>
						</dl>
						<div class="mt-6 flex justify-end">
							<Button variant="destructive" size="sm" onclick={() => onDelete?.(lb.arn)}>
								<Trash2Icon />
								Delete load balancer
							</Button>
						</div>
					</TabsContent>

					<TabsContent value="listeners" class="mt-4">
						{#if loading}
							<p class="text-xs text-muted-foreground">Loading…</p>
						{:else if listeners.length === 0}
							<p class="text-xs text-muted-foreground">No listeners attached.</p>
						{:else}
							<ul class="flex flex-col gap-1.5">
								{#each listeners as l (l.arn)}
									<li class="rounded-md border border-border bg-card/40 px-3 py-2">
										<div class="flex items-center gap-2 text-xs">
											<Badge variant="outline" class="h-4 px-1.5 text-[10px]">
												{l.protocol}:{l.port}
											</Badge>
											<span class="font-mono text-[10px] text-muted-foreground truncate">
												{l.arn}
											</span>
										</div>
									</li>
								{/each}
							</ul>
						{/if}
					</TabsContent>

					<TabsContent value="tags" class="mt-4">
						<div class="mb-3 grid grid-cols-[1fr_1fr_auto] gap-2">
							<div class="flex flex-col gap-1">
								<Label for="elb-tag-key" class="text-xs">Key</Label>
								<Input id="elb-tag-key" bind:value={newKey} class="h-8 text-xs" />
							</div>
							<div class="flex flex-col gap-1">
								<Label for="elb-tag-value" class="text-xs">Value</Label>
								<Input id="elb-tag-value" bind:value={newValue} class="h-8 text-xs" />
							</div>
							<Button
								class="self-end"
								size="sm"
								onclick={addTag}
								disabled={saving || !newKey.trim()}
							>
								<PlusIcon />
								Add
							</Button>
						</div>

						{#if tags.length === 0}
							<p class="text-xs text-muted-foreground">No tags.</p>
						{:else}
							<ul class="flex flex-col gap-1">
								{#each tags as t (t.key)}
									<li
										class="flex items-center justify-between rounded-md border border-border bg-card/40 px-3 py-1.5"
									>
										<span class="font-mono text-[11px]">
											<span class="text-muted-foreground">{t.key}</span>
											= {t.value}
										</span>
										<Button
											size="xs"
											variant="ghost"
											class="text-destructive hover:text-destructive"
											onclick={() => deleteTag(t.key)}
										>
											<Trash2Icon />
										</Button>
									</li>
								{/each}
							</ul>
						{/if}
					</TabsContent>
				</Tabs>
			</div>
		{/if}
	</SheetContent>
</Sheet>
