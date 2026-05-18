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
	import { Textarea } from '$lib/components/ui/textarea';
	import { Badge } from '$lib/components/ui/badge';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import TargetIcon from '@lucide/svelte/icons/target';
	import { toast } from 'svelte-sonner';
	import { putEvents, listRules, testEventPattern } from '$lib/api/eventbridge';

	interface Props {
		open: boolean;
		busName: string;
		onOpenChange: (open: boolean) => void;
	}

	let { open, busName, onOpenChange }: Props = $props();

	let source = $state('my.app');
	let detailType = $state('MyEvent');
	let detail = $state(JSON.stringify({ key: 'value' }, null, 2));
	let resources = $state('');
	let sending = $state(false);
	let testing = $state(false);
	// null = not evaluated yet; [] = evaluated, nothing matched.
	let matched = $state<string[] | null>(null);
	let sent = $state(false);

	$effect(() => {
		// Reset the preview whenever the composed event changes.
		void source;
		void detailType;
		void detail;
		void resources;
		matched = null;
		sent = false;
	});

	// The canonical envelope EventBridge rule patterns match against -
	// same shape the backend builds in PutEvents.
	function buildEventJson(): string | null {
		let parsedDetail: unknown;
		try {
			parsedDetail = detail.trim() ? JSON.parse(detail) : {};
		} catch {
			toast.error('Detail is not valid JSON');
			return null;
		}
		return JSON.stringify({
			id: 'preview',
			source: source.trim(),
			'detail-type': detailType.trim(),
			detail: parsedDetail,
			resources: resources
				.split(',')
				.map((s) => s.trim())
				.filter(Boolean),
			account: '000000000000',
			region: 'us-east-1'
		});
	}

	async function evaluateMatches(): Promise<string[] | null> {
		const eventJson = buildEventJson();
		if (eventJson === null) return null;
		const rules = await listRules(busName);
		// The router only delivers to ENABLED rules with a pattern.
		const candidates = rules.filter((r) => r.eventPattern && r.state === 'ENABLED');
		const hits = await Promise.all(
			candidates.map((r) =>
				testEventPattern(r.eventPattern as string, eventJson)
					.then((ok) => (ok ? r.name : null))
					.catch(() => null)
			)
		);
		return hits.filter((n): n is string => !!n);
	}

	async function preview() {
		if (!source.trim() || !detailType.trim()) {
			toast.error('Source and detail-type are required.');
			return;
		}
		testing = true;
		try {
			matched = (await evaluateMatches()) ?? matched;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Pattern test failed');
		} finally {
			testing = false;
		}
	}

	async function send() {
		if (!source.trim() || !detailType.trim()) {
			toast.error('Source and detail-type are required.');
			return;
		}
		sending = true;
		try {
			const res = await putEvents([
				{
					source: source.trim(),
					detailType: detailType.trim(),
					detail,
					eventBusName: busName,
					resources: resources
						.split(',')
						.map((s) => s.trim())
						.filter(Boolean),
				},
			]);
			if (res.failedEntryCount > 0) {
				toast.error(`Event rejected (${res.failedEntryCount} failed entries)`);
				return;
			}
			// Show what it fanned out to instead of a bare toast.
			matched = (await evaluateMatches().catch(() => [])) ?? [];
			sent = true;
			toast.success(
				`Sent to ${busName} - matched ${matched.length} rule${matched.length === 1 ? '' : 's'}.`
			);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'PutEvents failed');
		} finally {
			sending = false;
		}
	}
</script>

<Dialog {open} {onOpenChange}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>Send event</DialogTitle>
			<DialogDescription>
				Publish a single event to <span class="font-mono">{busName}</span> and see which
				rules it routes to.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="grid grid-cols-2 gap-3">
				<div class="flex flex-col gap-1">
					<Label for="evb-evt-source">Source</Label>
					<Input id="evb-evt-source" bind:value={source} />
				</div>
				<div class="flex flex-col gap-1">
					<Label for="evb-evt-type">Detail type</Label>
					<Input id="evb-evt-type" bind:value={detailType} />
				</div>
			</div>
			<div class="flex flex-col gap-1">
				<Label for="evb-evt-resources">Resources (comma-separated ARNs)</Label>
				<Input id="evb-evt-resources" bind:value={resources} placeholder="optional" />
			</div>
			<div class="flex flex-col gap-1">
				<Label for="evb-evt-detail">Detail (JSON)</Label>
				<Textarea
					id="evb-evt-detail"
					bind:value={detail}
					rows={8}
					class="font-mono text-xs"
				/>
			</div>

			{#if matched !== null}
				<div
					class="rounded-md border p-3 text-xs {sent
						? 'border-emerald-500/40 bg-emerald-500/5'
						: 'border-border bg-muted/30'}"
				>
					<div class="mb-1.5 flex items-center gap-1.5 font-medium">
						<TargetIcon class="size-3.5 text-muted-foreground" />
						{sent ? 'Delivered' : 'Routing preview'} -
						{matched.length} matching rule{matched.length === 1 ? '' : 's'}
						on {busName}
					</div>
					{#if matched.length}
						<div class="flex flex-wrap gap-1.5">
							{#each matched as name (name)}
								<Badge variant="outline" class="gap-1 font-mono text-[11px]">
									<CheckCircle2 class="size-3 text-emerald-500" />
									{name}
								</Badge>
							{/each}
						</div>
					{:else}
						<p class="text-muted-foreground">
							No enabled rule on this bus matches this event - it would be delivered
							but routed nowhere.
						</p>
					{/if}
				</div>
			{/if}
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Close</Button>
			<Button variant="outline" onclick={preview} disabled={testing || sending}>
				{testing ? 'Testing…' : 'Preview matches'}
			</Button>
			<Button onclick={send} disabled={sending || testing}>
				{sending ? 'Sending…' : 'Send event'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
