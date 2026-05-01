<script lang="ts">
	import {
		createUser,
		createRole,
		createGroup,
		createPolicy,
	} from '$lib/api/iam';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle,
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import PolicyEditor from './policy-editor.svelte';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import { toast } from 'svelte-sonner';

	export type CreateKind = 'user' | 'role' | 'group' | 'policy';

	interface Props {
		open: boolean;
		kind: CreateKind;
		onOpenChange: (open: boolean) => void;
		onCreated: () => void;
	}

	let { open = $bindable(), kind, onOpenChange, onCreated }: Props = $props();

	// Per-kind form state. Keep all of them around so reopening a
	// dialog with the same kind preserves an in-progress draft.
	let userName = $state('');
	let userPath = $state('');

	let roleName = $state('');
	let roleDescription = $state('');
	let assumeRolePolicy = $state(
		JSON.stringify(
			{
				Version: '2012-10-17',
				Statement: [
					{
						Effect: 'Allow',
						Principal: { Service: 'lambda.amazonaws.com' },
						Action: 'sts:AssumeRole',
					},
				],
			},
			null,
			2
		)
	);

	let groupName = $state('');

	let policyName = $state('');
	let policyDescription = $state('');
	let policyDocument = $state(
		JSON.stringify(
			{
				Version: '2012-10-17',
				Statement: [
					{
						Effect: 'Allow',
						Action: ['s3:GetObject'],
						Resource: ['arn:aws:s3:::example-bucket/*'],
					},
				],
			},
			null,
			2
		)
	);

	let saving = $state(false);

	const titles: Record<CreateKind, string> = {
		user: 'Create user',
		role: 'Create role',
		group: 'Create group',
		policy: 'Create managed policy',
	};
	const descriptions: Record<CreateKind, string> = {
		user: 'Standalone IAM user. Add to groups or attach policies after creation.',
		role: 'IAM role with a trust policy. Define which principals may assume it.',
		group: 'IAM group. Attach policies to the group; users in it inherit them.',
		policy:
			'Customer managed policy. Reusable across users, roles, and groups via attach-policy.',
	};

	async function submit() {
		saving = true;
		try {
			if (kind === 'user') {
				if (!userName.trim()) throw new Error('Name is required');
				await createUser(userName.trim(), userPath.trim() || undefined);
			} else if (kind === 'role') {
				if (!roleName.trim()) throw new Error('Name is required');
				try {
					JSON.parse(assumeRolePolicy);
				} catch {
					throw new Error('Trust policy is not valid JSON');
				}
				await createRole(
					roleName.trim(),
					assumeRolePolicy,
					roleDescription.trim() || undefined
				);
			} else if (kind === 'group') {
				if (!groupName.trim()) throw new Error('Name is required');
				await createGroup(groupName.trim());
			} else if (kind === 'policy') {
				if (!policyName.trim()) throw new Error('Name is required');
				try {
					JSON.parse(policyDocument);
				} catch {
					throw new Error('Policy document is not valid JSON');
				}
				await createPolicy(
					policyName.trim(),
					policyDocument,
					policyDescription.trim() || undefined
				);
			}
			toast.success(`${titles[kind]} succeeded`);
			onCreated();
			onOpenChange(false);
			resetForm();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Create failed');
		} finally {
			saving = false;
		}
	}

	function resetForm() {
		if (kind === 'user') {
			userName = '';
			userPath = '';
		} else if (kind === 'role') {
			roleName = '';
			roleDescription = '';
		} else if (kind === 'group') {
			groupName = '';
		} else if (kind === 'policy') {
			policyName = '';
			policyDescription = '';
		}
	}
</script>

<Dialog bind:open onOpenChange={(v) => onOpenChange(v)}>
	<DialogContent class="max-w-2xl">
		<DialogHeader>
			<DialogTitle>{titles[kind]}</DialogTitle>
			<DialogDescription>{descriptions[kind]}</DialogDescription>
		</DialogHeader>

		<div class="space-y-4 py-2">
			{#if kind === 'user'}
				<div class="space-y-1.5">
					<Label for="cu-name">User name</Label>
					<Input id="cu-name" bind:value={userName} placeholder="alice" />
				</div>
				<div class="space-y-1.5">
					<Label for="cu-path">Path (optional)</Label>
					<Input id="cu-path" bind:value={userPath} placeholder="/" />
				</div>
			{:else if kind === 'role'}
				<div class="space-y-1.5">
					<Label for="cr-name">Role name</Label>
					<Input id="cr-name" bind:value={roleName} placeholder="lambda-execution" />
				</div>
				<div class="space-y-1.5">
					<Label for="cr-desc">Description (optional)</Label>
					<Input id="cr-desc" bind:value={roleDescription} placeholder="Allows Lambda…" />
				</div>
				<div class="space-y-1.5">
					<Label for="cr-policy">Trust policy (assume-role policy document)</Label>
					<PolicyEditor id="cr-policy" bind:value={assumeRolePolicy} rows={10} />
				</div>
			{:else if kind === 'group'}
				<div class="space-y-1.5">
					<Label for="cg-name">Group name</Label>
					<Input id="cg-name" bind:value={groupName} placeholder="developers" />
				</div>
			{:else if kind === 'policy'}
				<div class="space-y-1.5">
					<Label for="cp-name">Policy name</Label>
					<Input id="cp-name" bind:value={policyName} placeholder="ReadOnlyS3" />
				</div>
				<div class="space-y-1.5">
					<Label for="cp-desc">Description (optional)</Label>
					<Textarea id="cp-desc" bind:value={policyDescription} rows={2} />
				</div>
				<div class="space-y-1.5">
					<Label for="cp-doc">Policy document</Label>
					<PolicyEditor id="cp-doc" bind:value={policyDocument} rows={12} />
				</div>
			{/if}
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)} disabled={saving}>
				Cancel
			</Button>
			<Button onclick={submit} disabled={saving}>
				{#if saving}
					<Loader2 class="size-4 animate-spin" />
				{/if}
				Create
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
