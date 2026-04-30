<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { fetchSesSent, type SesSentEmail } from '$lib/api';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { EmptyState } from '$lib/components/service';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Mail from '@lucide/svelte/icons/mail';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';

	let emails = $state<SesSentEmail[]>([]);
	let loading = $state(false);
	let filter = $state('');
	let expanded = $state<string | null>(null);
	let view = $state<'text' | 'html' | 'raw'>('text');

	const filtered = $derived(
		filter.trim()
			? emails.filter((e) => {
					const q = filter.trim().toLowerCase();
					return (
						(e.subject ?? '').toLowerCase().includes(q) ||
						e.from.toLowerCase().includes(q) ||
						e.to.some((t) => t.toLowerCase().includes(q))
					);
				})
			: emails
	);

	async function load() {
		loading = true;
		try {
			const r = await fetchSesSent();
			emails = r.emails;
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load outbox');
		} finally {
			loading = false;
		}
	}

	function fmtDate(unix: number): string {
		return new Date(unix * 1000).toLocaleString();
	}

	function recipients(e: SesSentEmail): string {
		return [...e.to, ...e.cc, ...e.bcc].join(', ');
	}

	function preferredView(e: SesSentEmail): 'text' | 'html' | 'raw' {
		if (e.bodyText) return 'text';
		if (e.bodyHtml) return 'html';
		return 'raw';
	}

	function toggle(messageId: string, e: SesSentEmail) {
		if (expanded === messageId) {
			expanded = null;
		} else {
			expanded = messageId;
			view = preferredView(e);
		}
	}

	onMount(load);
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex items-center gap-2 border-b border-border px-4 py-3">
		<Input
			type="search"
			placeholder="Filter by subject, from, or recipient..."
			bind:value={filter}
			class="h-8 max-w-sm"
		/>
		<div class="flex-1"></div>
		<Badge variant="secondary">{filtered.length} of {emails.length}</Badge>
		<Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} title="Refresh">
			<RefreshCw class="size-3.5 {loading ? 'animate-spin' : ''}" />
		</Button>
	</div>
	<div class="min-h-0 flex-1 overflow-y-auto p-4">
		{#if loading && emails.length === 0}
			<p class="text-xs text-muted-foreground">Loading...</p>
		{:else if emails.length === 0}
			<EmptyState
				icon={Mail}
				title="No sent emails"
				description="Awsim captures everything sent via SES SendEmail / SendRawEmail. Try sending one with the Compose button or your SDK."
			/>
		{:else if filtered.length === 0}
			<p class="text-xs text-muted-foreground">No emails match "{filter}".</p>
		{:else}
			<ul class="space-y-1.5">
				{#each filtered as e (e.messageId)}
					<li class="rounded border border-border/60">
						<button
							type="button"
							class="flex w-full items-start gap-2 px-3 py-2 text-left text-sm hover:bg-muted/40"
							onclick={() => toggle(e.messageId, e)}
							aria-expanded={expanded === e.messageId}
						>
							<ChevronRight
								class="mt-0.5 size-3.5 shrink-0 text-muted-foreground transition-transform {expanded ===
								e.messageId
									? 'rotate-90'
									: ''}"
							/>
							<div class="min-w-0 flex-1">
								<div class="flex flex-wrap items-baseline gap-2">
									<span class="truncate font-medium">{e.subject ?? '(no subject)'}</span>
									<span class="text-xs text-muted-foreground">{fmtDate(e.sentAt)}</span>
								</div>
								<div class="truncate text-xs text-muted-foreground">
									<span class="font-mono">{e.from}</span>
									<span> → </span>
									<span class="font-mono">{recipients(e)}</span>
								</div>
							</div>
							<Badge variant="outline" class="shrink-0 font-mono text-[10px]">{e.region}</Badge>
						</button>
						{#if expanded === e.messageId}
							<div class="space-y-3 border-t border-border/60 px-3 py-3">
								<dl class="grid grid-cols-[80px_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
									<dt class="text-muted-foreground">Message ID</dt>
									<dd class="truncate font-mono">{e.messageId}</dd>
									<dt class="text-muted-foreground">From</dt>
									<dd class="truncate font-mono">{e.from}</dd>
									<dt class="text-muted-foreground">To</dt>
									<dd class="truncate font-mono">{e.to.join(', ') || '—'}</dd>
									{#if e.cc.length > 0}
										<dt class="text-muted-foreground">Cc</dt>
										<dd class="truncate font-mono">{e.cc.join(', ')}</dd>
									{/if}
									{#if e.bcc.length > 0}
										<dt class="text-muted-foreground">Bcc</dt>
										<dd class="truncate font-mono">{e.bcc.join(', ')}</dd>
									{/if}
									<dt class="text-muted-foreground">Account</dt>
									<dd class="font-mono">{e.account}</dd>
								</dl>

								<div class="flex flex-wrap gap-1.5">
									{#if e.bodyText}
										<button
											type="button"
											class="rounded border px-2 py-0.5 text-xs transition-colors {view === 'text'
												? 'border-primary bg-primary/15 text-primary'
												: 'border-border bg-background text-muted-foreground'}"
											onclick={() => (view = 'text')}
										>
											Text
										</button>
									{/if}
									{#if e.bodyHtml}
										<button
											type="button"
											class="rounded border px-2 py-0.5 text-xs transition-colors {view === 'html'
												? 'border-primary bg-primary/15 text-primary'
												: 'border-border bg-background text-muted-foreground'}"
											onclick={() => (view = 'html')}
										>
											HTML
										</button>
									{/if}
									{#if e.raw}
										<button
											type="button"
											class="rounded border px-2 py-0.5 text-xs transition-colors {view === 'raw'
												? 'border-primary bg-primary/15 text-primary'
												: 'border-border bg-background text-muted-foreground'}"
											onclick={() => (view = 'raw')}
										>
											Raw
										</button>
									{/if}
								</div>

								{#if view === 'text' && e.bodyText}
									<pre class="max-h-96 overflow-auto rounded border border-border bg-muted/30 px-3 py-2 font-mono text-xs whitespace-pre-wrap">{e.bodyText}</pre>
								{:else if view === 'html' && e.bodyHtml}
									<iframe
										title="HTML body for {e.messageId}"
										sandbox=""
										srcdoc={e.bodyHtml}
										class="h-96 w-full rounded border border-border bg-white"
									></iframe>
								{:else if view === 'raw' && e.raw}
									<pre class="max-h-96 overflow-auto rounded border border-border bg-muted/30 px-3 py-2 font-mono text-xs whitespace-pre-wrap">{e.raw}</pre>
								{:else}
									<p class="text-xs text-muted-foreground">No body for this view.</p>
								{/if}
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
