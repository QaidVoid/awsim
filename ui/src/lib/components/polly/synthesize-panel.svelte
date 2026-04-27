<script lang="ts">
	import { onMount } from 'svelte';
	import { describeVoices, synthesizeSpeech, type Voice } from '$lib/api/polly';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
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
</script>

<div class="flex flex-col gap-4 p-4">
	<div class="flex flex-col gap-1">
		<Label for="polly-text">Text</Label>
		<Textarea id="polly-text" bind:value={text} rows={4} placeholder="Enter text to synthesize…" />
	</div>

	<div class="flex flex-wrap items-end gap-3">
		<div class="flex min-w-48 flex-col gap-1">
			<Label for="polly-voice">Voice</Label>
			<select
				id="polly-voice"
				bind:value={voiceId}
				class="h-9 rounded-md border border-input bg-background px-2 text-xs"
			>
				{#each voices as v (`${v.id}-${v.languageCode}`)}
					<option value={v.id}>
						{v.name} — {v.languageCode} ({v.gender})
					</option>
				{:else}
					<option value="">No voices available</option>
				{/each}
			</select>
		</div>

		<div class="flex flex-col gap-1">
			<Label for="polly-format">Format</Label>
			<select
				id="polly-format"
				bind:value={format}
				class="h-9 rounded-md border border-input bg-background px-2 text-xs"
			>
				<option value="mp3">mp3</option>
				<option value="ogg_vorbis">ogg_vorbis</option>
				<option value="pcm">pcm</option>
			</select>
		</div>

		<Button onclick={synth} disabled={busy || !text.trim() || !voiceId}>
			<PlayIcon />
			{busy ? 'Synthesizing…' : 'Synthesize'}
		</Button>
	</div>

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
