<script lang="ts">
	import { goto } from '$app/navigation';
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import UserPoolList from '$lib/components/cognito/user-pool-list.svelte';
	import IdentityPoolList from '$lib/components/cognito/identity-pool-list.svelte';
	import JwtDecoder from '$lib/components/cognito/jwt-decoder.svelte';
	import type { UserPool } from '$lib/api/cognito';

	let active = $state('userpools');

	function openPool(p: UserPool) {
		void goto(`/cognito/${p.id}`);
	}
</script>

<ServicePage
	title="Cognito"
	description="User pools, identity pools, and tooling for Cognito-based authentication."
>
	<Tabs bind:value={active} class="flex h-full min-h-0 flex-col">
		<div class="border-b border-border px-6 pt-3">
			<TabsList variant="line">
				<TabsTrigger value="userpools">User Pools</TabsTrigger>
				<TabsTrigger value="identitypools">Identity Pools</TabsTrigger>
				<TabsTrigger value="jwt">JWT decoder</TabsTrigger>
			</TabsList>
		</div>

		<TabsContent
			value="userpools"
			class="min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
		>
			<UserPoolList onSelect={openPool} />
		</TabsContent>
		<TabsContent
			value="identitypools"
			class="min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
		>
			<IdentityPoolList />
		</TabsContent>
		<TabsContent value="jwt" class="min-h-0 flex-1 overflow-y-auto data-[state=inactive]:hidden">
			<JwtDecoder />
		</TabsContent>
	</Tabs>
</ServicePage>
