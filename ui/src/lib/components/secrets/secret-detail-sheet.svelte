<script lang="ts">
	import {
		describeSecret,
		getSecretValue,
		listSecretVersions,
		putSecretValue,
		deleteSecret,
		type Secret,
		type SecretDetail,
		type SecretVersion
	} from '$lib/api/secrets';
	import { ConfirmDialog } from '$lib/components/ui/confirm-dialog';
	import Trash2 from '@lucide/svelte/icons/trash-2';
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
	import Eye from '@lucide/svelte/icons/eye';
	import EyeOff from '@lucide/svelte/icons/eye-off';
	import { toast } from 'svelte-sonner';

	interface Props {
		secret: Secret | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
		/** Notify the list to reload (after a delete). */
		onChanged?: () => void;
	}

	let { secret, open = $bindable(), onOpenChange, onChanged }: Props = $props();

	let confirmDelete = $state(false);
	let deleting = $state(false);

	async function doDelete() {
		if (!secret) return;
		deleting = true;
		try {
			// Force-delete: a dev tool wants it gone now, not staged for
			// recovery.
			await deleteSecret(secret.arn, true);
			toast.success('Secret deleted');
			confirmDelete = false;
			onOpenChange(false);
			onChanged?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete secret');
		} finally {
			deleting = false;
		}
	}

	let detail = $state<SecretDetail | null>(null);
	let versions = $state<SecretVersion[]>([]);
	let secretValue = $state<string | null>(null);
	let revealed = $state(false);
	let loading = $state(false);
	let saving = $state(false);
	let active = $state('value');
	let editValue = $state('');

	$effect(() => {
		if (secret && open) load(secret);
	});

	async function load(s: Secret) {
		detail = null;
		versions = [];
		secretValue = null;
		revealed = false;
		editValue = '';
		loading = true;
		try {
			const [d, v, val] = await Promise.all([
				describeSecret(s.arn),
				listSecretVersions(s.arn),
				getSecretValue(s.arn).catch(() => null)
			]);
			detail = d;
			versions = v;
			secretValue = val?.secretString ?? null;
			editValue = secretValue ?? '';
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load');
		} finally {
			loading = false;
		}
	}

	async function loadVersionValue(versionId: string) {
		if (!secret) return;
		try {
			const v = await getSecretValue(secret.arn, versionId);
			secretValue = v.secretString ?? null;
			editValue = secretValue ?? '';
			toast.success(`Loaded version ${versionId.slice(0, 8)}...`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load version');
		}
	}

	async function save() {
		if (!secret) return;
		saving = true;
		try {
			await putSecretValue(secret.arn, editValue);
			secretValue = editValue;
			versions = await listSecretVersions(secret.arn);
			toast.success('New version saved');
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save');
		} finally {
			saving = false;
		}
	}

	const masked = $derived(secretValue ? '•'.repeat(Math.min(secretValue.length, 32)) : '');
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-2xl overflow-y-auto sm:max-w-2xl">
		<SheetHeader>
			<div class="flex items-start justify-between gap-3">
				<div class="min-w-0">
					<SheetTitle>{secret?.name ?? ''}</SheetTitle>
					<SheetDescription class="truncate font-mono text-xs">
						{secret?.arn ?? ''}
					</SheetDescription>
				</div>
				<Button
					variant="ghost"
					size="sm"
					class="shrink-0 text-destructive hover:bg-destructive/10"
					onclick={() => (confirmDelete = true)}
				>
					<Trash2 class="size-3.5" />
					Delete
				</Button>
			</div>
		</SheetHeader>
		<div class="px-6 pb-6">
			<Tabs bind:value={active} class="mt-2">
				<TabsList variant="line">
					<TabsTrigger value="value">Value</TabsTrigger>
					<TabsTrigger value="versions">Versions ({versions.length})</TabsTrigger>
					<TabsTrigger value="rotation">Rotation</TabsTrigger>
				</TabsList>

				<TabsContent value="value" class="mt-4">
					{#if loading}
						<p class="text-xs text-muted-foreground">Loading...</p>
					{:else if secretValue === null}
						<p class="text-xs text-muted-foreground">No secret value (or access denied).</p>
					{:else}
						<div class="mb-2 flex items-center justify-between">
							<Label
								for="secret-value"
								class="text-xs uppercase tracking-wide text-muted-foreground"
								>Current value</Label
							>
							<Button variant="ghost" size="xs" onclick={() => (revealed = !revealed)}>
								{#if revealed}<EyeOff class="size-3" /> Hide{:else}<Eye class="size-3" /> Reveal{/if}
							</Button>
						</div>
						<Textarea
							id="secret-value"
							value={revealed ? editValue : masked}
							readonly={!revealed}
							oninput={(e) => {
								if (revealed) editValue = (e.currentTarget as HTMLTextAreaElement).value;
							}}
							rows={6}
							class="font-mono text-xs"
						/>
						{#if revealed}
							<div class="mt-2 flex justify-end">
								<Button size="sm" onclick={save} disabled={saving}>
									{saving ? 'Saving...' : 'Save as new version'}
								</Button>
							</div>
						{/if}
					{/if}
				</TabsContent>

				<TabsContent value="versions" class="mt-4">
					{#if versions.length === 0}
						<p class="text-xs text-muted-foreground">No versions found.</p>
					{:else}
						<ul class="space-y-1.5">
							{#each versions as v (v.versionId)}
								<li
									class="flex items-center justify-between rounded border border-border/60 px-3 py-2"
								>
									<div class="min-w-0">
										<div class="flex items-center gap-2 font-mono text-xs">
											{v.versionId.slice(0, 16)}...
											{#each v.stages as s (s)}
												<Badge variant="outline">{s}</Badge>
											{/each}
										</div>
										{#if v.createdDate}
											<div class="text-xs text-muted-foreground">{v.createdDate}</div>
										{/if}
									</div>
									<Button variant="ghost" size="xs" onclick={() => loadVersionValue(v.versionId)}>
										Load
									</Button>
								</li>
							{/each}
						</ul>
					{/if}
				</TabsContent>

				<TabsContent value="rotation" class="mt-4">
					<dl class="grid grid-cols-3 gap-x-4 gap-y-2 text-sm">
						<dt class="text-muted-foreground">Rotation enabled</dt>
						<dd class="col-span-2">
							{#if detail?.rotationEnabled}
								<Badge variant="secondary">enabled</Badge>
							{:else}
								<Badge variant="outline">disabled</Badge>
							{/if}
						</dd>
						{#if detail?.rotationLambdaArn}
							<dt class="text-muted-foreground">Rotation lambda</dt>
							<dd class="col-span-2 break-all font-mono text-xs">{detail.rotationLambdaArn}</dd>
						{/if}
						{#if detail?.rotationRules?.automaticallyAfterDays}
							<dt class="text-muted-foreground">Frequency</dt>
							<dd class="col-span-2">every {detail.rotationRules.automaticallyAfterDays} days</dd>
						{/if}
						{#if detail?.kmsKeyId}
							<dt class="text-muted-foreground">KMS key</dt>
							<dd class="col-span-2 font-mono text-xs">{detail.kmsKeyId}</dd>
						{/if}
						<dt class="text-muted-foreground">Last changed</dt>
						<dd class="col-span-2">{detail?.lastChangedDate ?? '—'}</dd>
					</dl>
				</TabsContent>
			</Tabs>
		</div>
	</SheetContent>
</Sheet>

<ConfirmDialog
	bind:open={confirmDelete}
	title="Delete secret?"
	description={`Permanently delete "${secret?.name ?? ''}" and all its versions.`}
	busy={deleting}
	onConfirm={doDelete}
	onClose={() => (confirmDelete = false)}
/>
