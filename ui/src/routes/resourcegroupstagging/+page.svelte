<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import ResourceTagList from '$lib/components/resourcegroupstagging/resource-tag-list.svelte';
</script>

<ServicePage
	title="Resource Groups Tagging"
	description="Cross-service resource discovery by tag. Lists every resource tagged through the Resource Groups Tagging API."
>
	<ResourceTagList />
</ServicePage>
