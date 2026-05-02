<script lang="ts">
	import { useTab } from '$lib/util/tab.svelte';
	import { ServicePage } from '$lib/components/service';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import { Label } from '$lib/components/ui/label';
	import WebAclsTab from '$lib/components/waf/web-acls-tab.svelte';
	import RuleGroupsTab from '$lib/components/waf/rule-groups-tab.svelte';
	import IpSetsTab from '$lib/components/waf/ip-sets-tab.svelte';
	import type { WafScope } from '$lib/api/waf';

	let scope = $state<WafScope>('REGIONAL');
	let active: string = $state(
		useTab('waf', ['webacls', 'rulegroups', 'ipsets'] as const, 'webacls', {
			get: (): string => active,
			set: (v) => (active = v)
		})
	);
</script>

<ServicePage
	title="WAF v2"
	description="Web Application Firewall — Web ACLs, rule groups, and IP sets across REGIONAL and CLOUDFRONT scopes."
>
	{#snippet toolbar()}
		<Label for="waf-scope" class="text-xs uppercase tracking-wide text-muted-foreground"
			>Scope</Label
		>
		<select
			id="waf-scope"
			bind:value={scope}
			class="h-8 rounded-md border border-input bg-background px-2 text-xs shadow-xs"
		>
			<option value="REGIONAL">REGIONAL</option>
			<option value="CLOUDFRONT">CLOUDFRONT</option>
		</select>
	{/snippet}

	<Tabs bind:value={active} class="flex h-full min-h-0 flex-col">
		<div class="border-b border-border px-6 pt-3">
			<TabsList variant="line">
				<TabsTrigger value="webacls">Web ACLs</TabsTrigger>
				<TabsTrigger value="rulegroups">Rule Groups</TabsTrigger>
				<TabsTrigger value="ipsets">IP Sets</TabsTrigger>
			</TabsList>
		</div>

		<TabsContent
			value="webacls"
			class="min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
		>
			<WebAclsTab {scope} />
		</TabsContent>
		<TabsContent
			value="rulegroups"
			class="min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
		>
			<RuleGroupsTab {scope} />
		</TabsContent>
		<TabsContent
			value="ipsets"
			class="min-h-0 flex-1 overflow-hidden data-[state=inactive]:hidden"
		>
			<IpSetsTab {scope} />
		</TabsContent>
	</Tabs>
</ServicePage>
