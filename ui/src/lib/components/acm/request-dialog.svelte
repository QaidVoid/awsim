<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { requestCertificate } from '$lib/api/acm';
	import { toast } from 'svelte-sonner';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		onCreated: () => void;
	}

	let { open = $bindable(), onOpenChange, onCreated }: Props = $props();

	let domain = $state('');
	let sans = $state('');
	let validation = $state<'DNS' | 'EMAIL'>('DNS');
	let busy = $state(false);

	async function submit() {
		if (!domain.trim()) {
			toast.error('Domain is required');
			return;
		}
		busy = true;
		try {
			const sanList = sans
				.split(',')
				.map((s) => s.trim())
				.filter(Boolean);
			await requestCertificate(domain.trim(), {
				sans: sanList.length ? sanList : undefined,
				validationMethod: validation
			});
			toast.success('Certificate requested');
			domain = '';
			sans = '';
			onOpenChange(false);
			onCreated();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Request failed');
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(v) => onOpenChange(v)}>
	<DialogContent>
		<DialogHeader>
			<DialogTitle>Request a certificate</DialogTitle>
			<DialogDescription>
				ACM will validate ownership of the domain via the chosen method.
			</DialogDescription>
		</DialogHeader>
		<div class="grid gap-3 py-2">
			<div class="flex flex-col gap-1.5">
				<Label for="cert-domain" class="text-xs">Primary domain</Label>
				<Input id="cert-domain" bind:value={domain} placeholder="example.com" />
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="cert-sans" class="text-xs">Subject alt names (comma separated)</Label>
				<Input
					id="cert-sans"
					bind:value={sans}
					placeholder="www.example.com, api.example.com"
					class="font-mono text-xs"
				/>
			</div>
			<div class="flex flex-col gap-1.5">
				<Label for="cert-validation" class="text-xs">Validation method</Label>
				<Select
					type="single"
					value={validation}
					onValueChange={(v) => (validation = v as 'DNS' | 'EMAIL')}
				>
					<SelectTrigger id="cert-validation" class="w-full">
						{validation}
					</SelectTrigger>
					<SelectContent>
						<SelectItem value="DNS" label="DNS">DNS</SelectItem>
						<SelectItem value="EMAIL" label="EMAIL">EMAIL</SelectItem>
					</SelectContent>
				</Select>
			</div>
		</div>
		<DialogFooter>
			<Button variant="ghost" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={busy || !domain.trim()}>
				{busy ? 'Requesting...' : 'Request'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
