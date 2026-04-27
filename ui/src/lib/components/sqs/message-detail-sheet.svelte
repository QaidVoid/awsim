<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetHeader,
		SheetTitle,
		SheetDescription,
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { toast } from 'svelte-sonner';
	import { deleteMessage, type Message } from '$lib/api/sqs';

	interface Props {
		open: boolean;
		queueUrl: string;
		message: Message | null;
		onOpenChange: (open: boolean) => void;
		onDeleted?: (id: string) => void;
	}

	let { open, queueUrl, message, onOpenChange, onDeleted }: Props = $props();

	let deleting = $state(false);

	function prettyBody(body: string): string {
		try {
			return JSON.stringify(JSON.parse(body), null, 2);
		} catch {
			return body;
		}
	}

	async function copy(text: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast.success('Copied.');
		} catch {
			toast.error('Copy failed.');
		}
	}

	async function handleDelete() {
		if (!message) return;
		deleting = true;
		try {
			await deleteMessage(queueUrl, message.receiptHandle);
			toast.success('Message deleted.');
			onDeleted?.(message.messageId);
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete');
		} finally {
			deleting = false;
		}
	}
</script>

<Sheet {open} {onOpenChange}>
	<SheetContent side="right" class="w-full sm:max-w-xl">
		<SheetHeader>
			<SheetTitle>Message details</SheetTitle>
			<SheetDescription>
				{#if message}
					<span class="font-mono text-xs">{message.messageId}</span>
				{/if}
			</SheetDescription>
		</SheetHeader>

		{#if message}
			<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4">
				<div class="flex flex-wrap gap-1">
					{#each Object.entries(message.attributes) as [key, value] (key)}
						<Badge variant="outline" class="h-5 px-2 text-[10px] font-mono">
							{key}: {value}
						</Badge>
					{/each}
				</div>

				<section>
					<div class="mb-2 flex items-center justify-between">
						<h3 class="text-xs font-semibold uppercase text-muted-foreground">Body</h3>
						<Button variant="ghost" size="xs" onclick={() => copy(message?.body ?? '')}>
							<CopyIcon />
							Copy
						</Button>
					</div>
					<pre
						class="max-h-[40vh] overflow-auto rounded-md border border-border bg-muted/40 p-3 text-xs font-mono whitespace-pre-wrap break-all">{prettyBody(
							message.body
						)}</pre>
				</section>

				{#if Object.keys(message.messageAttributes).length > 0}
					<section>
						<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">
							Message attributes
						</h3>
						<dl class="grid grid-cols-[140px_1fr] gap-x-3 gap-y-1 text-xs">
							{#each Object.entries(message.messageAttributes) as [name, attr] (name)}
								<dt class="font-mono text-muted-foreground">{name}</dt>
								<dd>
									<span class="font-mono">{attr.stringValue ?? attr.binaryValue ?? ''}</span>
									<span class="ml-1 text-[10px] text-muted-foreground">({attr.dataType})</span>
								</dd>
							{/each}
						</dl>
					</section>
				{/if}

				<section>
					<h3 class="mb-2 text-xs font-semibold uppercase text-muted-foreground">
						Receipt handle
					</h3>
					<pre
						class="max-h-32 overflow-auto rounded-md border border-border bg-muted/40 p-2 text-[10px] font-mono break-all">{message.receiptHandle}</pre>
				</section>
			</div>

			<div class="flex justify-end gap-2 border-t border-border px-4 py-3">
				<Button variant="outline" onclick={() => onOpenChange(false)}>Close</Button>
				<Button variant="destructive" onclick={handleDelete} disabled={deleting}>
					<Trash2Icon />
					{deleting ? 'Deleting…' : 'Delete'}
				</Button>
			</div>
		{/if}
	</SheetContent>
</Sheet>
