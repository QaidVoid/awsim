<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { createIdentityProvider, type IdpType } from '$lib/api/cognito';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import KvEditor from './kv-editor.svelte';
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		open: boolean;
		poolId: string;
		onClose: () => void;
		onCreated: () => void;
	}

	let { open = $bindable(false), poolId, onClose, onCreated }: Props = $props();

	const TYPES: IdpType[] = [
		'OIDC',
		'SAML',
		'Google',
		'Facebook',
		'SignInWithApple',
		'LoginWithAmazon'
	];

	/// Per-type required ProviderDetails keys, surfaced as datalist hints.
	const SUGGESTED_KEYS: Record<IdpType, string[]> = {
		Google: ['client_id', 'client_secret', 'authorize_scopes'],
		Facebook: ['client_id', 'client_secret', 'authorize_scopes'],
		LoginWithAmazon: ['client_id', 'client_secret', 'authorize_scopes'],
		SignInWithApple: ['client_id', 'team_id', 'key_id', 'private_key', 'authorize_scopes'],
		OIDC: [
			'client_id',
			'client_secret',
			'oidc_issuer',
			'authorize_scopes',
			'attributes_request_method',
			'authorize_url',
			'token_url',
			'attributes_url',
			'jwks_uri'
		],
		SAML: ['MetadataURL', 'MetadataFile', 'IDPSignout', 'RequestSigningAlgorithm']
	};

	let name = $state('');
	let type = $state<IdpType>('OIDC');
	let details = $state<{ key: string; value: string }[]>([]);
	let attrs = $state<{ key: string; value: string }[]>([]);
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (!open) {
			name = '';
			type = 'OIDC';
			details = [];
			attrs = [];
			saving = false;
			error = null;
		}
	});

	async function submit() {
		if (!name.trim()) {
			error = 'Name is required';
			return;
		}
		const detailsMap: Record<string, string> = {};
		for (const d of details) if (d.key) detailsMap[d.key] = d.value;
		const attrsMap: Record<string, string> = {};
		for (const a of attrs) if (a.key) attrsMap[a.key] = a.value;
		saving = true;
		error = null;
		try {
			await createIdentityProvider({
				poolId,
				name: name.trim(),
				type,
				providerDetails: detailsMap,
				attributeMapping: attrsMap
			});
			toast.success(`Created ${name.trim()}`);
			onCreated();
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Create failed';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<DialogContent class="sm:max-w-2xl">
		<DialogHeader>
			<DialogTitle>Create identity provider</DialogTitle>
			<DialogDescription>
				Federation source for sign-in. Type-specific fields are listed in the picker below.
			</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-4"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			<div class="grid gap-3 sm:grid-cols-2">
				<div class="space-y-1.5">
					<Label for="idp-name">Name</Label>
					<Input id="idp-name" bind:value={name} autocomplete="off" />
				</div>
				<div class="space-y-1.5">
					<Label for="idp-type">Type</Label>
					<Select
						type="single"
						value={type}
						onValueChange={(v) => (type = v as IdpType)}
					>
						<SelectTrigger id="idp-type" class="w-full text-sm">
							{type}
						</SelectTrigger>
						<SelectContent>
							{#each TYPES as t (t)}
								<SelectItem value={t} label={t}>{t}</SelectItem>
							{/each}
						</SelectContent>
					</Select>
				</div>
			</div>

			<div class="space-y-1.5">
				<Label class="text-xs uppercase tracking-wide text-muted-foreground">
					Provider details
				</Label>
				<KvEditor
					bind:entries={details}
					keyPlaceholder="e.g. client_id"
					valuePlaceholder="value"
					suggestedKeys={SUGGESTED_KEYS[type]}
					onChange={(e) => (details = e)}
				/>
			</div>

			<div class="space-y-1.5">
				<Label class="text-xs uppercase tracking-wide text-muted-foreground">
					Attribute mapping (optional)
				</Label>
				<p class="text-[11px] text-muted-foreground">
					Maps IdP attribute names to Cognito user attributes (e.g. <code>email</code> →
					<code>email</code>).
				</p>
				<KvEditor
					bind:entries={attrs}
					keyPlaceholder="cognito attr (e.g. email)"
					valuePlaceholder="idp attr"
					onChange={(e) => (attrs = e)}
				/>
			</div>

			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{/if}
			<DialogFooter>
				<Button type="button" variant="outline" onclick={onClose} disabled={saving}>
					Cancel
				</Button>
				<Button type="submit" disabled={saving || !name.trim()}>
					{#if saving}<Loader2 class="size-3.5 animate-spin" />{/if}
					Create
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
