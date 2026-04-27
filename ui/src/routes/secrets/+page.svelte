<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import SecretList from '$lib/components/secrets/secret-list.svelte';
</script>

<ServicePage
	title="Secrets Manager"
	description="Securely store, retrieve, and rotate API keys, database credentials and other secrets."
>
	<SecretList />
</ServicePage>
