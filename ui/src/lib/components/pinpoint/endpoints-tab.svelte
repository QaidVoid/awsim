<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { toast } from 'svelte-sonner';
	import { updateEndpoint } from '$lib/api/pinpoint';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { EmptyState } from '$lib/components/service';
	import SmartphoneIcon from '@lucide/svelte/icons/smartphone';

	interface Props {
		appId: string;
	}

	let { appId }: Props = $props();

	let newId = $state('');
	let newChannel = $state<'EMAIL' | 'SMS' | 'PUSH' | 'CUSTOM'>('EMAIL');
	let newAddress = $state('');
	let upserting = $state(false);

	async function upsert() {
		if (!newId.trim() || !newAddress.trim())
			return toast.error('EndpointId and Address are required.');
		upserting = true;
		try {
			await updateEndpoint({
				appId,
				endpointId: newId.trim(),
				channelType: newChannel,
				address: newAddress.trim()
			});
			toast.success(`Upserted endpoint ${newId.trim()}.`);
			newId = '';
			newAddress = '';
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to upsert endpoint');
		} finally {
			upserting = false;
		}
	}
</script>

<div class="flex flex-col gap-3 p-4">
	<div class="space-y-2 rounded-md border border-border p-3">
		<div class="text-xs font-semibold">Upsert endpoint</div>
		<div class="grid grid-cols-3 gap-2">
			<Input bind:value={newId} placeholder="endpoint ID" class="h-8 text-xs col-span-2 font-mono" />
			<Select
				type="single"
				value={newChannel}
				onValueChange={(v) => (newChannel = v as 'EMAIL' | 'SMS' | 'PUSH' | 'CUSTOM')}
			>
				<SelectTrigger size="sm" class="w-full text-xs">
					{newChannel}
				</SelectTrigger>
				<SelectContent>
					<SelectItem value="EMAIL" label="EMAIL">EMAIL</SelectItem>
					<SelectItem value="SMS" label="SMS">SMS</SelectItem>
					<SelectItem value="PUSH" label="PUSH">PUSH</SelectItem>
					<SelectItem value="CUSTOM" label="CUSTOM">CUSTOM</SelectItem>
				</SelectContent>
			</Select>
		</div>
		<Input
			bind:value={newAddress}
			placeholder="address (email / phone / device token)"
			class="h-8 text-xs font-mono"
		/>
		<Button size="sm" onclick={upsert} disabled={upserting}>
			<PlusIcon />
			{upserting ? 'Saving…' : 'Upsert endpoint'}
		</Button>
	</div>

	<EmptyState
		icon={SmartphoneIcon}
		title="Endpoint listing is by id"
		description="Pinpoint doesn't expose ListEndpoints — fetch one via the AWS CLI (aws pinpoint get-endpoint --application-id ... --endpoint-id ...). The form above upserts (PUT)."
	/>
</div>
