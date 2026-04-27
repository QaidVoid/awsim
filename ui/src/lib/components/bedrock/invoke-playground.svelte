<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listFoundationModels,
		invokeModel,
		type FoundationModel,
	} from '$lib/api/bedrock';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import SendIcon from '@lucide/svelte/icons/send';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import UserIcon from '@lucide/svelte/icons/user';
	import { toast } from 'svelte-sonner';

	interface Turn {
		id: number;
		role: 'user' | 'assistant';
		text: string;
		raw?: string;
		error?: boolean;
	}

	let models = $state<FoundationModel[]>([]);
	let modelId = $state<string>('');
	let prompt = $state('');
	let turns = $state<Turn[]>([]);
	let busy = $state(false);
	let nextId = 0;

	let textModels = $derived(
		models.filter(
			(m) => m.inputModalities.includes('TEXT') && m.outputModalities.includes('TEXT')
		)
	);

	onMount(async () => {
		try {
			models = await listFoundationModels();
			if (!modelId && textModels.length > 0) {
				modelId = textModels[0].modelId;
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load models');
		}
	});

	function buildBody(provider: string, text: string): unknown {
		const p = provider.toLowerCase();
		if (p === 'anthropic') {
			return {
				anthropic_version: 'bedrock-2023-05-31',
				max_tokens: 1024,
				messages: [{ role: 'user', content: text }],
			};
		}
		if (p === 'amazon') {
			return {
				inputText: text,
				textGenerationConfig: { maxTokenCount: 1024, temperature: 0.7 },
			};
		}
		if (p === 'meta') {
			return { prompt: text, max_gen_len: 512, temperature: 0.7 };
		}
		if (p === 'cohere') {
			return { prompt: text, max_tokens: 512, temperature: 0.7 };
		}
		return { prompt: text, max_tokens: 512 };
	}

	function extractText(provider: string, raw: string): string {
		try {
			const data = JSON.parse(raw) as Record<string, unknown>;
			const p = provider.toLowerCase();
			if (p === 'anthropic') {
				const content = data.content as
					| Array<{ type?: string; text?: string }>
					| undefined;
				if (Array.isArray(content)) {
					return content
						.filter((c) => c?.type === 'text')
						.map((c) => c.text ?? '')
						.join('\n');
				}
			}
			if (p === 'amazon') {
				const results = data.results as Array<{ outputText?: string }> | undefined;
				if (Array.isArray(results)) {
					return results.map((r) => r.outputText ?? '').join('\n');
				}
			}
			if (p === 'meta' && typeof data.generation === 'string') {
				return data.generation;
			}
			if (p === 'cohere') {
				const generations = data.generations as Array<{ text?: string }> | undefined;
				if (Array.isArray(generations)) {
					return generations.map((g) => g.text ?? '').join('\n');
				}
			}
			if (typeof data.completion === 'string') return data.completion;
			if (typeof data.outputText === 'string') return data.outputText;
		} catch {
			// fall through
		}
		return raw;
	}

	async function send() {
		const text = prompt.trim();
		if (!text || !modelId || busy) return;
		const model = models.find((m) => m.modelId === modelId);
		if (!model) {
			toast.error('Select a model first.');
			return;
		}
		busy = true;
		const userTurn: Turn = { id: ++nextId, role: 'user', text };
		turns = [...turns, userTurn];
		prompt = '';
		try {
			const body = buildBody(model.providerName, text);
			const result = await invokeModel(modelId, body);
			const out = extractText(model.providerName, result.body);
			turns = [
				...turns,
				{ id: ++nextId, role: 'assistant', text: out, raw: result.body },
			];
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'InvokeModel failed';
			turns = [...turns, { id: ++nextId, role: 'assistant', text: msg, error: true }];
			toast.error(msg);
		} finally {
			busy = false;
		}
	}

	function handleKey(e: KeyboardEvent) {
		if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
			e.preventDefault();
			send();
		}
	}

	function clearChat() {
		turns = [];
	}
</script>

<div class="flex h-full min-h-0 flex-col gap-3 p-4">
	<div class="flex flex-wrap items-end gap-3">
		<div class="flex min-w-64 flex-1 flex-col gap-1">
			<Label for="bedrock-model-select">Model</Label>
			<select
				id="bedrock-model-select"
				bind:value={modelId}
				class="h-9 rounded-md border border-input bg-background px-2 text-xs"
			>
				{#each textModels as m (m.modelId)}
					<option value={m.modelId}>{m.providerName} — {m.modelName}</option>
				{:else}
					<option value="">No text-to-text models</option>
				{/each}
			</select>
		</div>
		<Button variant="outline" size="sm" onclick={clearChat} disabled={turns.length === 0}>
			<Trash2Icon />
			Clear
		</Button>
	</div>

	<div class="min-h-0 flex-1 overflow-y-auto rounded-md border border-border bg-muted/20 p-3">
		{#if turns.length === 0}
			<EmptyState
				icon={SparklesIcon}
				title="Invoke playground"
				description="Send a prompt to a foundation model. Ctrl/Cmd+Enter to submit."
			/>
		{:else}
			<div class="flex flex-col gap-3">
				{#each turns as turn (turn.id)}
					<div class="flex gap-2">
						<div class="mt-0.5 shrink-0">
							{#if turn.role === 'user'}
								<UserIcon class="size-4 text-muted-foreground" />
							{:else}
								<SparklesIcon
									class={'size-4 ' +
										(turn.error ? 'text-destructive' : 'text-primary')}
								/>
							{/if}
						</div>
						<div class="min-w-0 flex-1">
							<div class="mb-0.5 flex items-center gap-2">
								<span class="text-[10px] font-semibold uppercase text-muted-foreground">
									{turn.role}
								</span>
								{#if turn.error}
									<Badge variant="destructive" class="h-4 px-1 text-[10px]">error</Badge>
								{/if}
							</div>
							<pre
								class={'overflow-auto rounded-md border border-border bg-background p-2 text-xs whitespace-pre-wrap break-words ' +
									(turn.error ? 'text-destructive' : '')}>{turn.text}</pre>
						</div>
					</div>
				{/each}
			</div>
		{/if}
	</div>

	<div class="flex flex-col gap-2">
		<Label for="bedrock-prompt">Prompt</Label>
		<Textarea
			id="bedrock-prompt"
			bind:value={prompt}
			rows={3}
			placeholder="Ask the model anything…"
			onkeydown={handleKey}
			disabled={busy}
		/>
		<div class="flex items-center justify-between text-[10px] text-muted-foreground">
			<span>Ctrl/Cmd+Enter to send</span>
			<Button size="sm" onclick={send} disabled={busy || !prompt.trim() || !modelId}>
				<SendIcon />
				{busy ? 'Sending…' : 'Send'}
			</Button>
		</div>
	</div>
</div>
