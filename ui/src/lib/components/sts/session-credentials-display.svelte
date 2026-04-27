<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { toast } from 'svelte-sonner';
	import type { Credentials } from '$lib/api/sts';

	interface Props {
		credentials: Credentials | null;
	}

	let { credentials }: Props = $props();

	async function copy(value: string, label: string) {
		try {
			await navigator.clipboard.writeText(value);
			toast.success(`${label} copied`);
		} catch {
			toast.error('Clipboard unavailable');
		}
	}

	function envExport(c: Credentials): string {
		return [
			`export AWS_ACCESS_KEY_ID="${c.accessKeyId}"`,
			`export AWS_SECRET_ACCESS_KEY="${c.secretAccessKey}"`,
			`export AWS_SESSION_TOKEN="${c.sessionToken}"`
		].join('\n');
	}
</script>

{#if credentials}
	<div class="mt-4 flex flex-col gap-3 rounded-md border border-border bg-card/40 p-4">
		<p class="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
			Temporary credentials
		</p>
		<dl class="flex flex-col gap-2 text-xs">
			{#each [{ label: 'Access key ID', val: credentials.accessKeyId }, { label: 'Secret access key', val: credentials.secretAccessKey }, { label: 'Session token', val: credentials.sessionToken }, { label: 'Expiration', val: credentials.expiration }] as item (item.label)}
				<div class="flex items-start gap-2">
					<dt class="w-32 shrink-0 text-muted-foreground">{item.label}</dt>
					<dd class="min-w-0 flex-1 break-all rounded bg-muted/40 px-2 py-1 font-mono text-[11px]">
						{item.val}
					</dd>
					<Button
						size="xs"
						variant="ghost"
						aria-label={`Copy ${item.label}`}
						onclick={() => copy(item.val, item.label)}
					>
						<CopyIcon />
					</Button>
				</div>
			{/each}
		</dl>
		<div class="flex justify-end">
			<Button size="xs" variant="outline" onclick={() => copy(envExport(credentials), 'Env exports')}>
				<CopyIcon /> Copy as env exports
			</Button>
		</div>
	</div>
{/if}
