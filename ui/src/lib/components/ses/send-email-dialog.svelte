<script lang="ts">
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import { Textarea } from '$lib/components/ui/textarea';
	import SendIcon from '@lucide/svelte/icons/send';
	import { toast } from 'svelte-sonner';
	import { sendEmail, listIdentities, type Identity } from '$lib/api/ses';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { open = $bindable(), onOpenChange }: Props = $props();

	let identities = $state<Identity[]>([]);
	let from = $state('');
	let to = $state('');
	let cc = $state('');
	let bcc = $state('');
	let subject = $state('');
	let html = $state('');
	let text = $state('');
	let body = $state<'html' | 'text'>('html');
	let sending = $state(false);

	$effect(() => {
		if (open) loadIdentities();
	});

	async function loadIdentities() {
		try {
			identities = await listIdentities();
			if (!from && identities.length > 0) from = identities[0].name;
		} catch {
			// non-fatal: user can still type a from address manually
		}
	}

	function splitAddrs(s: string): string[] {
		return s
			.split(',')
			.map((x) => x.trim())
			.filter(Boolean);
	}

	async function send() {
		if (!from.trim()) {
			toast.error('From address is required.');
			return;
		}
		const toAddrs = splitAddrs(to);
		if (toAddrs.length === 0) {
			toast.error('At least one recipient is required.');
			return;
		}
		if (!subject.trim()) {
			toast.error('Subject is required.');
			return;
		}
		const hasHtml = body === 'html' && html.trim() !== '';
		const hasText = body === 'text' && text.trim() !== '';
		if (!hasHtml && !hasText) {
			toast.error('Provide an HTML or text body.');
			return;
		}
		sending = true;
		try {
			const result = await sendEmail({
				fromEmailAddress: from.trim(),
				toAddresses: toAddrs,
				ccAddresses: cc ? splitAddrs(cc) : undefined,
				bccAddresses: bcc ? splitAddrs(bcc) : undefined,
				subject: subject.trim(),
				html: body === 'html' ? html : undefined,
				text: body === 'text' ? text : undefined
			});
			toast.success(`Sent (${result.messageId.slice(0, 16)}…)`);
			to = '';
			cc = '';
			bcc = '';
			subject = '';
			html = '';
			text = '';
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Send failed');
		} finally {
			sending = false;
		}
	}
</script>

<Dialog open={open} onOpenChange={(o) => onOpenChange(o)}>
	<DialogContent class="sm:max-w-2xl">
		<DialogHeader>
			<DialogTitle>Compose email</DialogTitle>
			<DialogDescription>
				Sends a one-off SES message. Recipients accept comma-separated lists.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3 px-4">
			<div class="flex flex-col gap-1">
				<Label for="ses-from">From</Label>
				{#if identities.length > 0}
					<Select type="single" bind:value={from}>
						<SelectTrigger id="ses-from" class="w-full font-mono text-xs">
							{from}
						</SelectTrigger>
						<SelectContent>
							{#each identities as i (i.name)}
								<SelectItem value={i.name} label={i.name}>{i.name}</SelectItem>
							{/each}
						</SelectContent>
					</Select>
				{:else}
					<Input
						id="ses-from"
						bind:value={from}
						placeholder="sender@example.com"
						class="font-mono text-xs"
					/>
				{/if}
			</div>

			<div class="grid gap-3 sm:grid-cols-2">
				<div class="flex flex-col gap-1">
					<Label for="ses-to">To</Label>
					<Input id="ses-to" bind:value={to} placeholder="user@example.com" />
				</div>
				<div class="flex flex-col gap-1">
					<Label for="ses-cc">Cc</Label>
					<Input id="ses-cc" bind:value={cc} />
				</div>
			</div>

			<div class="flex flex-col gap-1">
				<Label for="ses-bcc">Bcc</Label>
				<Input id="ses-bcc" bind:value={bcc} />
			</div>

			<div class="flex flex-col gap-1">
				<Label for="ses-subject">Subject</Label>
				<Input id="ses-subject" bind:value={subject} />
			</div>

			<Tabs bind:value={body}>
				<TabsList variant="line">
					<TabsTrigger value="html">HTML</TabsTrigger>
					<TabsTrigger value="text">Text</TabsTrigger>
				</TabsList>
				<TabsContent value="html" class="mt-2">
					<Label for="ses-body-html" class="sr-only">HTML body</Label>
					<Textarea
						id="ses-body-html"
						bind:value={html}
						rows={8}
						class="font-mono text-xs"
						placeholder="<h1>Hello</h1>"
					/>
				</TabsContent>
				<TabsContent value="text" class="mt-2">
					<Label for="ses-body-text" class="sr-only">Text body</Label>
					<Textarea
						id="ses-body-text"
						bind:value={text}
						rows={8}
						class="font-mono text-xs"
						placeholder="Hello"
					/>
				</TabsContent>
			</Tabs>
		</div>

		<DialogFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={send} disabled={sending}>
				<SendIcon /> {sending ? 'Sending…' : 'Send'}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
