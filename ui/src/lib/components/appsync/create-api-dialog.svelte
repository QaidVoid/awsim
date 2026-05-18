<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { toast } from 'svelte-sonner';
	import { createGraphqlApi } from '$lib/api/appsync';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated: () => void;
	}

	let { open, onOpenChange, onCreated }: Props = $props();

	const AUTH_TYPES = ['API_KEY', 'AWS_IAM', 'AMAZON_COGNITO_USER_POOLS'] as const;
	type AuthType = (typeof AUTH_TYPES)[number];

	let name = $state('');
	let authType = $state<AuthType>('API_KEY');
	let creating = $state(false);

	function reset() {
		name = '';
		authType = 'API_KEY';
	}

	async function submit() {
		if (!name.trim()) return;
		creating = true;
		try {
			await createGraphqlApi({ name: name.trim(), authenticationType: authType });
			toast.success(`API ${name.trim()} created.`);
			reset();
			onOpenChange(false);
			onCreated();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Create failed');
		} finally {
			creating = false;
		}
	}
</script>

<Dialog
	{open}
	onOpenChange={(o) => {
		if (!o) reset();
		onOpenChange(o);
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Create GraphQL API</DialogTitle>
			<DialogDescription>Define the API name and authentication mode.</DialogDescription>
		</DialogHeader>
		<div class="flex flex-col gap-3">
			<div class="flex flex-col gap-1">
				<Label for="appsync-create-name">Name</Label>
				<Input id="appsync-create-name" bind:value={name} placeholder="my-graphql-api" />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="appsync-create-auth">Authentication</Label>
				<Select
					type="single"
					value={authType}
					onValueChange={(v) => (authType = v as AuthType)}
				>
					<SelectTrigger id="appsync-create-auth" class="w-full">
						{authType}
					</SelectTrigger>
					<SelectContent>
						{#each AUTH_TYPES as t (t)}
							<SelectItem value={t} label={t}>{t}</SelectItem>
						{/each}
					</SelectContent>
				</Select>
			</div>
		</div>
		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={creating || !name.trim()}>
				{creating ? 'Creating…' : 'Create'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
