<script lang="ts">
	import { onMount } from 'svelte';
	import { describeVoices, synthesizeSpeech, type Voice } from '$lib/api/polly';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { Badge } from '$lib/components/ui/badge';
	import PlayIcon from '@lucide/svelte/icons/play';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import { toast } from 'svelte-sonner';

	type Format = 'mp3' | 'ogg_vorbis' | 'pcm';

	let voices = $state<Voice[]>([]);
	let voiceId = $state<string>('Joanna');
	let text = $state('Hello from AWSim Polly!');
	let format = $state<Format>('mp3');
	let busy = $state(false);
	let audioUrl = $state<string | null>(null);
	let lastChars = $state<number | null>(null);

	onMount(async () => {
		try {
			voices = await describeVoices();
			if (voices.length > 0 && !voices.some((v) => v.id === voiceId)) {
				voiceId = voices[0].id;
			}
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load voices');
		}
	});

	function revoke() {
		if (audioUrl) {
			URL.revokeObjectURL(audioUrl);
			audioUrl = null;
		}
	}

	async function synth() {
		if (!text.trim() || !voiceId || busy) return;
		revoke();
		busy = true;
		try {
			const res = await synthesizeSpeech({
				text,
				voiceId,
				outputFormat: format,
			});
			audioUrl = URL.createObjectURL(res.audio);
			lastChars = res.requestCharacters;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Synthesis failed');
		} finally {
			busy = false;
		}
	}

	function download() {
		if (!audioUrl) return;
		const ext = format === 'ogg_vorbis' ? 'ogg' : format;
		const a = document.createElement('a');
		a.href = audioUrl;
		a.download = `polly-${voiceId}.${ext}`;
		a.click();
	}

	let voiceLabel = $derived.by(() => {
		const v = voices.find((x) => x.id === voiceId);
		return v ? `${v.name} — ${v.languageCode} (${v.gender})` : '';
	});
</script>

<div class="flex flex-col gap-4 p-4">
	<div class="flex flex-col gap-1">
		<Label for="polly-text">Text</Label>
		<Textarea id="polly-text" bind:value={text} rows={4} placeholder="Enter text to synthesize…" />
	</div>

	<div class="flex flex-wrap items-end gap-3">
		<div class="flex min-w-48 flex-col gap-1">
			<Label for="polly-voice">Voice</Label>
			<Select type="single" bind:value={voiceId} disabled={voices.length === 0}>
				<SelectTrigger id="polly-voice" class="w-full text-xs">
					{voiceId ? voiceLabel : 'No voices available'}
				</SelectTrigger>
				<SelectContent>
					{#each voices as v (`${v.id}-${v.languageCode}`)}
						<SelectItem
							value={v.id}
							label={`${v.name} — ${v.languageCode} (${v.gender})`}
						>
							{v.name} — {v.languageCode} ({v.gender})
						</SelectItem>
					{/each}
				</SelectContent>
			</Select>
		</div>

		<div class="flex flex-col gap-1">
			<Label for="polly-format">Format</Label>
			<Select
				type="single"
				value={format}
				onValueChange={(v) => (format = v as Format)}
			>
				<SelectTrigger id="polly-format" class="w-[140px] text-xs">
					{format}
				</SelectTrigger>
				<SelectContent>
					<SelectItem value="mp3" label="mp3">mp3</SelectItem>
					<SelectItem value="ogg_vorbis" label="ogg_vorbis">ogg_vorbis</SelectItem>
					<SelectItem value="pcm" label="pcm">pcm</SelectItem>
				</SelectContent>
			</Select>
		</div>

		<Button onclick={synth} disabled={busy || !text.trim() || !voiceId}>
			<PlayIcon />
			{busy ? 'Synthesizing…' : 'Synthesize'}
		</Button>
	</div>

	{#if busy}
		<p class="text-xs text-muted-foreground">Synthesizing…</p>
	{/if}

	{#if audioUrl}
		<div class="flex flex-col gap-2 rounded-md border border-border bg-muted/20 p-3">
			<div class="flex items-center justify-between">
				<div class="flex items-center gap-2 text-xs text-muted-foreground">
					<Badge variant="secondary" class="h-4 px-1 text-[10px]">{format}</Badge>
					{#if lastChars !== null}
						<span>{lastChars} chars billed</span>
					{/if}
				</div>
				<Button variant="outline" size="sm" onclick={download}>
					<DownloadIcon />
					Download
				</Button>
			</div>
			<!-- svelte-ignore a11y_media_has_caption -->
			<audio controls src={audioUrl} class="w-full"></audio>
		</div>
	{/if}
</div>
