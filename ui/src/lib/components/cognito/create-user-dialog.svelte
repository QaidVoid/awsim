<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { adminCreateUser, type SchemaAttribute, type UserPoolDetail } from '$lib/api/cognito';
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
	import Loader2 from '@lucide/svelte/icons/loader-2';

	interface Props {
		open: boolean;
		poolId: string;
		pool: UserPoolDetail | null;
		onClose: () => void;
		onCreated: () => void;
	}

	let { open = $bindable(false), poolId, pool, onClose, onCreated }: Props = $props();

	// Pool attribute config drives which fields we show. When the
	// pool's UsernameAttributes includes `email` (or `phone_number`),
	// real Cognito uses that attribute as the canonical Username -
	// the SDK refuses to accept any other Username string. We mirror
	// that: hide the standalone Username field and use email/phone
	// as the Username.
	let usernameMode = $derived<'email' | 'phone_number' | 'free'>(
		pool?.usernameAttributes?.includes('email')
			? 'email'
			: pool?.usernameAttributes?.includes('phone_number')
				? 'phone_number'
				: 'free'
	);

	// All standard attrs the user might want to populate. We surface
	// email and phone always; the rest are reachable via the dynamic
	// attribute editor below.
	let username = $state('');
	let temporaryPassword = $state('');
	let email = $state('');
	let phone = $state('');
	let suppressInvite = $state(true);
	let saving = $state(false);
	let error = $state<string | null>(null);

	// Schema-driven custom + extra-standard fields. Keyed by the
	// attribute name (including `custom:` prefix for custom attrs).
	let extraValues = $state<Record<string, string>>({});

	let extraAttrs = $derived<SchemaAttribute[]>(
		(pool?.schemaAttributes ?? []).filter((a) => {
			// Skip the always-rendered staples (handled by their own inputs)
			// and the auto-managed ones the user can never set directly.
			if (a.name === 'sub') return false;
			if (a.name === 'email_verified' || a.name === 'phone_number_verified') return false;
			if (a.name === 'updated_at') return false;
			if (a.name === 'email' || a.name === 'phone_number') return false;
			// In email-username mode the email field already covers `email`,
			// and same for phone. Don't double-render.
			return true;
		})
	);

	$effect(() => {
		if (!open) {
			username = '';
			temporaryPassword = '';
			email = '';
			phone = '';
			suppressInvite = true;
			saving = false;
			error = null;
			extraValues = {};
		}
	});

	function constraintHint(a: SchemaAttribute): string | null {
		if (a.type === 'String' && a.stringConstraints) {
			const { minLength, maxLength } = a.stringConstraints;
			if (minLength !== undefined && maxLength !== undefined) {
				return `${minLength}-${maxLength} chars`;
			}
			if (maxLength !== undefined) return `<= ${maxLength} chars`;
			if (minLength !== undefined) return `>= ${minLength} chars`;
		}
		if (a.type === 'Number' && a.numberConstraints) {
			const { minValue, maxValue } = a.numberConstraints;
			if (minValue !== undefined && maxValue !== undefined) {
				return `${minValue}..${maxValue}`;
			}
			if (maxValue !== undefined) return `<= ${maxValue}`;
			if (minValue !== undefined) return `>= ${minValue}`;
		}
		if (a.type === 'Boolean') return "'true' or 'false'";
		if (a.type === 'DateTime') return 'ISO-8601 or epoch';
		return null;
	}

	async function submit() {
		// Resolve the effective Username based on the pool's config.
		const effectiveUsername = (() => {
			if (usernameMode === 'email') return email.trim();
			if (usernameMode === 'phone_number') return phone.trim();
			return username.trim();
		})();

		if (!effectiveUsername) {
			error =
				usernameMode === 'email'
					? 'Email is required (this pool uses email as the username).'
					: usernameMode === 'phone_number'
						? 'Phone number is required (this pool uses phone as the username).'
						: 'Username is required';
			return;
		}

		// Required-attr check before sending so the user gets a fast
		// inline error rather than waiting for the API to bounce it.
		const required = (pool?.schemaAttributes ?? []).filter(
			(a) => a.required && a.name !== 'sub'
		);
		const missing = required
			.map((a) => a.name)
			.filter((n) => {
				if (n === 'email') return !email.trim();
				if (n === 'phone_number') return !phone.trim();
				return !(extraValues[n] ?? '').trim();
			});
		if (missing.length > 0) {
			error = `Missing required attribute(s): ${missing.join(', ')}`;
			return;
		}

		saving = true;
		error = null;
		const attrs: { name: string; value: string }[] = [];

		// Standard attrs surfaced as their own inputs.
		if (email.trim()) attrs.push({ name: 'email', value: email.trim() });
		if (phone.trim()) attrs.push({ name: 'phone_number', value: phone.trim() });

		// Dynamic attrs (custom + standard not directly surfaced).
		for (const a of extraAttrs) {
			const v = (extraValues[a.name] ?? '').trim();
			if (v) attrs.push({ name: a.name, value: v });
		}

		try {
			await adminCreateUser({
				poolId,
				username: effectiveUsername,
				temporaryPassword: temporaryPassword.trim() || undefined,
				attributes: attrs.length > 0 ? attrs : undefined,
				messageAction: suppressInvite ? 'SUPPRESS' : undefined
			});
			toast.success(`Created ${effectiveUsername}`);
			onCreated();
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Create user failed';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Create user</DialogTitle>
			<DialogDescription>
				{#if usernameMode === 'email'}
					This pool uses email as the username. Admin-created users start in
					FORCE_CHANGE_PASSWORD.
				{:else if usernameMode === 'phone_number'}
					This pool uses phone number as the username. Admin-created users start in
					FORCE_CHANGE_PASSWORD.
				{:else}
					Admin-created users start in FORCE_CHANGE_PASSWORD.
				{/if}
			</DialogDescription>
		</DialogHeader>
		<form
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			{#if usernameMode === 'free'}
				<div class="flex flex-col gap-1.5">
					<Label for="user-name">Username</Label>
					<Input id="user-name" bind:value={username} placeholder="alice" autocomplete="off" />
				</div>
			{/if}

			<div class="flex flex-col gap-1.5">
				<Label for="user-pw">Temporary password (optional)</Label>
				<Input
					id="user-pw"
					type="text"
					bind:value={temporaryPassword}
					placeholder="leave blank to auto-generate"
					autocomplete="off"
				/>
			</div>

			<div class="flex flex-col gap-1.5">
				<Label for="user-email">
					Email{#if usernameMode === 'email' || pool?.schemaAttributes?.find((a) => a.name === 'email')?.required}
						<span class="text-destructive">*</span>
					{/if}
				</Label>
				<Input
					id="user-email"
					type="email"
					bind:value={email}
					placeholder="alice@example.com"
					autocomplete="off"
				/>
			</div>

			<div class="flex flex-col gap-1.5">
				<Label for="user-phone">
					Phone{#if usernameMode === 'phone_number' || pool?.schemaAttributes?.find((a) => a.name === 'phone_number')?.required}
						<span class="text-destructive">*</span>
					{/if}
				</Label>
				<Input
					id="user-phone"
					bind:value={phone}
					placeholder="+15551234567"
					autocomplete="off"
				/>
			</div>

			{#if extraAttrs.length > 0}
				<details class="rounded border border-border bg-muted/30 p-2">
					<summary class="cursor-pointer text-xs font-medium text-muted-foreground">
						Additional attributes ({extraAttrs.length})
					</summary>
					<div class="mt-2 flex flex-col gap-2.5">
						{#each extraAttrs as attr (attr.name)}
							<div class="flex flex-col gap-1">
								<Label for={`attr-${attr.name}`} class="text-xs">
									{attr.name}
									<span class="text-muted-foreground">({attr.type})</span>
									{#if attr.required}<span class="text-destructive">*</span>{/if}
									{#if !attr.mutable}<span class="text-muted-foreground">(immutable)</span>{/if}
								</Label>
								<Input
									id={`attr-${attr.name}`}
									bind:value={extraValues[attr.name]}
									placeholder={constraintHint(attr) ?? ''}
									autocomplete="off"
								/>
							</div>
						{/each}
					</div>
				</details>
			{/if}

			<label class="flex items-center gap-2 text-xs text-muted-foreground">
				<input type="checkbox" bind:checked={suppressInvite} class="size-3.5" />
				Suppress invitation message (MessageAction=SUPPRESS)
			</label>
			{#if error}
				<p class="text-xs text-destructive">{error}</p>
			{/if}
			<DialogFooter>
				<Button type="button" variant="outline" onclick={onClose} disabled={saving}>
					Cancel
				</Button>
				<Button type="submit" disabled={saving}>
					{#if saving}
						<Loader2 class="size-3.5 animate-spin" />
					{/if}
					Create
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
