<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import CallerIdentityCard from '$lib/components/sts/caller-identity-card.svelte';
	import AssumeRoleForm from '$lib/components/sts/assume-role-form.svelte';
	import FederationTokenForm from '$lib/components/sts/federation-token-form.svelte';
	import DecodeAuthorizationMessageForm from '$lib/components/sts/decode-authorization-message-form.svelte';
</script>

<ServicePage
	title="STS"
	description="Security Token Service: caller identity and temporary credentials."
>
	<div class="grid gap-4 p-4 lg:grid-cols-2">
		<div class="lg:col-span-2">
			<CallerIdentityCard />
		</div>
		<AssumeRoleForm />
		<FederationTokenForm />
		<div class="lg:col-span-2">
			<DecodeAuthorizationMessageForm />
		</div>
	</div>
</ServicePage>
