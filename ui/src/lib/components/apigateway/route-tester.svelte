<script lang="ts">
	import { stageInvokeUrl, getStages, type Stage } from '$lib/api/apigateway';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { Textarea } from '$lib/components/ui/textarea';
	import { toast } from 'svelte-sonner';
	import Copy from '@lucide/svelte/icons/copy';
	import Send from '@lucide/svelte/icons/send';

	interface Props {
		restApiId: string;
	}

	let { restApiId }: Props = $props();

	let stages = $state<Stage[]>([]);
	let stage = $state('');
	let method = $state<'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH'>('GET');
	let path = $state('/');
	let body = $state('');
	let response = $state<string | null>(null);
	let status = $state<number | null>(null);
	let sending = $state(false);

	async function loadStages() {
		try {
			stages = await getStages(restApiId);
			if (!stage && stages[0]) stage = stages[0].stageName;
		} catch {
			stages = [];
		}
	}

	$effect(() => {
		if (restApiId) {
			stage = '';
			loadStages();
		}
	});

	let invokeUrl = $derived(stage ? `${stageInvokeUrl(restApiId, stage)}${path}` : '');

	let curl = $derived.by(() => {
		if (!invokeUrl) return '';
		const parts = ['curl', '-X', method, `"${invokeUrl}"`];
		if (body && (method === 'POST' || method === 'PUT' || method === 'PATCH')) {
			parts.push('-H', `'Content-Type: application/json'`);
			parts.push('-d', `'${body.replace(/'/g, "'\\''")}'`);
		}
		return parts.join(' ');
	});

	async function send() {
		if (!invokeUrl) {
			toast.error('Pick a stage first.');
			return;
		}
		sending = true;
		response = null;
		status = null;
		try {
			const init: RequestInit = { method };
			if (body && (method === 'POST' || method === 'PUT' || method === 'PATCH')) {
				init.body = body;
				init.headers = { 'Content-Type': 'application/json' };
			}
			const res = await fetch(invokeUrl, init);
			status = res.status;
			response = await res.text();
		} catch (e) {
			response = e instanceof Error ? e.message : 'Request failed';
			status = 0;
		} finally {
			sending = false;
		}
	}

	async function copyCurl() {
		if (!curl) return;
		try {
			await navigator.clipboard.writeText(curl);
			toast.success('curl command copied');
		} catch {
			toast.error('Copy failed');
		}
	}
</script>

<div class="flex h-full min-h-0 flex-col gap-3 p-4">
	<div class="grid grid-cols-[110px_140px_1fr] gap-2">
		<div class="flex flex-col gap-1">
			<Label for="rt-method">Method</Label>
			<Select
				type="single"
				value={method}
				onValueChange={(v) => (method = v as 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH')}
			>
				<SelectTrigger id="rt-method" size="sm" class="w-full text-xs">
					{method}
				</SelectTrigger>
				<SelectContent>
					<SelectItem value="GET" label="GET">GET</SelectItem>
					<SelectItem value="POST" label="POST">POST</SelectItem>
					<SelectItem value="PUT" label="PUT">PUT</SelectItem>
					<SelectItem value="PATCH" label="PATCH">PATCH</SelectItem>
					<SelectItem value="DELETE" label="DELETE">DELETE</SelectItem>
				</SelectContent>
			</Select>
		</div>
		<div class="flex flex-col gap-1">
			<Label for="rt-stage">Stage</Label>
			<Select type="single" bind:value={stage}>
				<SelectTrigger id="rt-stage" size="sm" class="w-full text-xs">
					{stage ? stage : '- pick -'}
				</SelectTrigger>
				<SelectContent>
					{#each stages as s (s.stageName)}
						<SelectItem value={s.stageName} label={s.stageName}>{s.stageName}</SelectItem>
					{/each}
				</SelectContent>
			</Select>
		</div>
		<div class="flex flex-col gap-1">
			<Label for="rt-path">Path</Label>
			<Input id="rt-path" bind:value={path} class="h-8 font-mono text-xs" />
		</div>
	</div>

	{#if method === 'POST' || method === 'PUT' || method === 'PATCH'}
		<div class="flex flex-col gap-1">
			<Label for="rt-body">Body (JSON)</Label>
			<Textarea
				id="rt-body"
				bind:value={body}
				rows={4}
				class="font-mono text-xs"
				placeholder={'{\n  "key": "value"\n}'}
			/>
		</div>
	{/if}

	<div class="flex flex-wrap items-center gap-2">
		<Button size="sm" onclick={send} disabled={sending || !invokeUrl}>
			<Send />
			{sending ? 'Sending...' : 'Send'}
		</Button>
		<Button
			size="sm"
			variant="outline"
			onclick={copyCurl}
			disabled={!curl}
			title="Copy curl"
		>
			<Copy />
			Copy curl
		</Button>
	</div>

	{#if curl}
		<div class="flex flex-col gap-1">
			<Label for="rt-curl">curl</Label>
			<pre
				id="rt-curl"
				class="overflow-x-auto rounded-md border border-border bg-background/40 p-2 font-mono text-[11px]">{curl}</pre>
		</div>
	{/if}

	{#if response !== null}
		<div class="flex min-h-0 flex-1 flex-col gap-1">
			<div class="flex items-center gap-2">
				<Label class="text-xs">Response</Label>
				{#if status !== null}
					<span
						class="rounded px-1.5 py-0.5 text-[10px] font-medium {status >= 200 &&
						status < 300
							? 'bg-emerald-500/15 text-emerald-400'
							: 'bg-red-500/15 text-red-400'}"
					>
						{status}
					</span>
				{/if}
			</div>
			<pre
				class="min-h-0 flex-1 overflow-auto rounded-md border border-border bg-background/40 p-2 font-mono text-[11px]">{response}</pre>
		</div>
	{/if}
</div>
