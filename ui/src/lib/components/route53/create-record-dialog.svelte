<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { Textarea } from '$lib/components/ui/textarea';
	import { toast } from 'svelte-sonner';
	import { changeResourceRecordSets } from '$lib/api/route53';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		hostedZoneId: string;
		zoneName: string;
		onCreated?: () => void;
	}

	let { open, onOpenChange, hostedZoneId, zoneName, onCreated }: Props = $props();

	const TYPES = ['A', 'AAAA', 'CNAME', 'MX', 'TXT', 'NS', 'SRV', 'PTR'];

	let name = $state('');
	let type = $state<string>('A');
	let ttl = $state(300);
	let values = $state('');
	let saving = $state(false);

	function reset() {
		name = '';
		type = 'A';
		ttl = 300;
		values = '';
	}

	function fqdn(input: string): string {
		const z = zoneName.endsWith('.') ? zoneName : `${zoneName}.`;
		const trimmed = input.trim();
		if (!trimmed) return z;
		if (trimmed === '@') return z;
		const lower = trimmed.toLowerCase();
		const lowerZ = z.toLowerCase();
		if (lower.endsWith(lowerZ) || lower === lowerZ.slice(0, -1)) {
			return trimmed.endsWith('.') ? trimmed : `${trimmed}.`;
		}
		return `${trimmed}.${z}`;
	}

	async function submit() {
		const list = values
			.split(/\r?\n/)
			.map((v) => v.trim())
			.filter(Boolean);
		if (list.length === 0) {
			toast.error('At least one value is required.');
			return;
		}
		saving = true;
		try {
			await changeResourceRecordSets(hostedZoneId, [
				{
					action: 'UPSERT',
					name: fqdn(name),
					type,
					ttl,
					values: list,
				},
			]);
			toast.success('Record saved.');
			reset();
			onOpenChange(false);
			onCreated?.();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save record');
		} finally {
			saving = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>New record</DialogTitle>
			<DialogDescription>
				Adding to <span class="font-mono">{zoneName}</span>. Use UPSERT semantics.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="r53-record-name">Name</Label>
				<Input
					id="r53-record-name"
					bind:value={name}
					placeholder="www  (or leave blank for apex)"
					autocomplete="off"
				/>
				<p class="text-[11px] text-muted-foreground">Becomes <span class="font-mono">{fqdn(name)}</span></p>
			</div>
			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1">
					<Label for="r53-record-type">Type</Label>
					<Select type="single" bind:value={type}>
						<SelectTrigger id="r53-record-type" class="w-full">
							{type}
						</SelectTrigger>
						<SelectContent>
							{#each TYPES as t (t)}
								<SelectItem value={t} label={t}>{t}</SelectItem>
							{/each}
						</SelectContent>
					</Select>
				</div>
				<div class="flex flex-col gap-1">
					<Label for="r53-record-ttl">TTL</Label>
					<Input id="r53-record-ttl" type="number" min="0" bind:value={ttl} />
				</div>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="r53-record-values">Values (one per line)</Label>
				<Textarea id="r53-record-values" rows={4} bind:value={values} class="font-mono text-xs" />
			</div>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={submit} disabled={saving || !values.trim()}>
				{saving ? 'Saving…' : 'Save record'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
