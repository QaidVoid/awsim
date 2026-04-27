<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import CertificateList from '$lib/components/acm/certificate-list.svelte';
</script>

<ServicePage
	title="Certificate Manager (ACM)"
	description="Provision, manage, and inspect SSL/TLS certificates."
>
	<CertificateList />
</ServicePage>
