<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import ResourceTagList from '$lib/components/resourcegroupstagging/resource-tag-list.svelte';
</script>

<ServicePage
	title="Resource Groups Tagging"
	description="Cross-service resource discovery by tag. Resources are tracked here only when tagged via this API; per-service tag propagation is not modelled."
>
	<ResourceTagList />
</ServicePage>
