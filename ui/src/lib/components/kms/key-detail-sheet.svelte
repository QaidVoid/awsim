<script lang="ts">
	import {
		describeKey,
		getKeyPolicy,
		putKeyPolicy,
		getKeyRotationStatus,
		enableKeyRotation,
		disableKeyRotation,
		type Key,
		type KeyDetail
	} from '$lib/api/kms';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
	import { toast } from 'svelte-sonner';

	interface Props {
		k: Key | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { k, open = $bindable(), onOpenChange }: Props = $props();

	let detail = $state<KeyDetail | null>(null);
	let policyDoc = $state('');
	let rotation = $state(false);
	let loading = $state(false);
	let saving = $state(false);
	let active = $state('overview');

	$effect(() => {
		if (k && open) load(k);
	});

	async function load(key: Key) {
		detail = null;
		policyDoc = '';
		loading = true;
		try {
			const [d, p, r] = await Promise.all([
				describeKey(key.keyId),
				getKeyPolicy(key.keyId).catch(() => ''),
				getKeyRotationStatus(key.keyId).catch(() => false)
			]);
			detail = d;
			rotation = r;
			try {
				policyDoc = p ? JSON.stringify(JSON.parse(p), null, 2) : '';
			} catch {
				policyDoc = p;
			}
		} finally {
			loading = false;
		}
	}

	async function savePolicy() {
		if (!k) return;
		saving = true;
		try {
			await putKeyPolicy(k.keyId, policyDoc);
			toast.success('Key policy saved');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save');
		} finally {
			saving = false;
		}
	}

	async function toggleRotation() {
		if (!k) return;
		try {
			if (rotation) await disableKeyRotation(k.keyId);
			else await enableKeyRotation(k.keyId);
			rotation = !rotation;
			toast.success(`Rotation ${rotation ? 'enabled' : 'disabled'}`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Toggle failed');
		}
	}
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-2xl overflow-y-auto sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle class="font-mono text-sm">{k?.keyId ?? ''}</SheetTitle>
			<SheetDescription class="truncate font-mono text-xs">{k?.keyArn ?? ''}</SheetDescription>
		</SheetHeader>
		<div class="px-6 pb-6">
			<Tabs bind:value={active} class="mt-2">
				<TabsList variant="line">
					<TabsTrigger value="overview">Overview</TabsTrigger>
					<TabsTrigger value="policy">Policy</TabsTrigger>
					<TabsTrigger value="rotation">Rotation</TabsTrigger>
				</TabsList>

				<TabsContent value="overview" class="mt-4">
					{#if loading}
						<p class="text-xs text-muted-foreground">Loading...</p>
					{:else if detail}
						<dl class="grid grid-cols-3 gap-x-4 gap-y-2 text-sm">
							<dt class="text-muted-foreground">State</dt>
							<dd class="col-span-2"><Badge variant="outline">{detail.keyState}</Badge></dd>
							<dt class="text-muted-foreground">Enabled</dt>
							<dd class="col-span-2">
								{#if detail.enabled}
									<Badge variant="secondary">enabled</Badge>
								{:else}
									<Badge variant="destructive">disabled</Badge>
								{/if}
							</dd>
							{#if detail.description}
								<dt class="text-muted-foreground">Description</dt>
								<dd class="col-span-2">{detail.description}</dd>
							{/if}
							{#if detail.keyUsage}
								<dt class="text-muted-foreground">Usage</dt>
								<dd class="col-span-2">{detail.keyUsage}</dd>
							{/if}
							{#if detail.origin}
								<dt class="text-muted-foreground">Origin</dt>
								<dd class="col-span-2">{detail.origin}</dd>
							{/if}
							<dt class="text-muted-foreground">Created</dt>
							<dd class="col-span-2">{detail.creationDate}</dd>
						</dl>
					{/if}
				</TabsContent>

				<TabsContent value="policy" class="mt-4">
					<Label
						for="key-policy-doc"
						class="mb-1 block text-xs uppercase tracking-wide text-muted-foreground"
						>Policy document</Label
					>
					<Textarea
						id="key-policy-doc"
						bind:value={policyDoc}
						rows={18}
						class="font-mono text-xs"
					/>
					<div class="mt-2 flex justify-end">
						<Button size="sm" onclick={savePolicy} disabled={saving || !policyDoc}>
							{saving ? 'Saving...' : 'Save policy'}
						</Button>
					</div>
				</TabsContent>

				<TabsContent value="rotation" class="mt-4">
					<div class="flex items-center justify-between rounded border border-border/60 p-4">
						<div>
							<div class="text-sm font-medium">Automatic key rotation</div>
							<div class="text-xs text-muted-foreground">
								When enabled, AWS rotates the key every 365 days.
							</div>
						</div>
						<Button variant={rotation ? 'destructive' : 'outline'} size="sm" onclick={toggleRotation}>
							{rotation ? 'Disable rotation' : 'Enable rotation'}
						</Button>
					</div>
				</TabsContent>
			</Tabs>
		</div>
	</SheetContent>
</Sheet>
