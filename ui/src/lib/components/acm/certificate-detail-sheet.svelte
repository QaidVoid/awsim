<script lang="ts">
	import {
		describeCertificate,
		type Certificate,
		type CertificateDetail
	} from '$lib/api/acm';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { Badge } from '$lib/components/ui/badge';

	interface Props {
		cert: Certificate | null;
		open: boolean;
		onOpenChange: (open: boolean) => void;
	}

	let { cert, open = $bindable(), onOpenChange }: Props = $props();

	let detail = $state<CertificateDetail | null>(null);
	let loading = $state(false);

	$effect(() => {
		if (cert && open) load(cert);
	});

	async function load(c: Certificate) {
		detail = null;
		loading = true;
		try {
			detail = await describeCertificate(c.arn);
		} finally {
			loading = false;
		}
	}

	function statusVariant(s: string): 'secondary' | 'destructive' | 'outline' {
		if (s === 'ISSUED') return 'secondary';
		if (s === 'EXPIRED' || s === 'FAILED' || s === 'REVOKED' || s === 'VALIDATION_TIMED_OUT')
			return 'destructive';
		return 'outline';
	}
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-2xl overflow-y-auto sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>{cert?.domainName ?? ''}</SheetTitle>
			<SheetDescription class="truncate font-mono text-xs">{cert?.arn ?? ''}</SheetDescription>
		</SheetHeader>
		<div class="px-6 pb-6">
			{#if loading}
				<p class="text-xs text-muted-foreground">Loading...</p>
			{:else if detail}
				<dl class="grid grid-cols-3 gap-x-4 gap-y-2 py-4 text-sm">
					<dt class="text-muted-foreground">Status</dt>
					<dd class="col-span-2"
						><Badge variant={statusVariant(detail.status as string)}>{detail.status}</Badge></dd
					>
					{#if detail.type}
						<dt class="text-muted-foreground">Type</dt>
						<dd class="col-span-2">{detail.type}</dd>
					{/if}
					{#if detail.issuer}
						<dt class="text-muted-foreground">Issuer</dt>
						<dd class="col-span-2">{detail.issuer}</dd>
					{/if}
					{#if detail.keyAlgorithm}
						<dt class="text-muted-foreground">Key algorithm</dt>
						<dd class="col-span-2">{detail.keyAlgorithm}</dd>
					{/if}
					{#if detail.signatureAlgorithm}
						<dt class="text-muted-foreground">Signature algorithm</dt>
						<dd class="col-span-2">{detail.signatureAlgorithm}</dd>
					{/if}
					{#if detail.notBefore}
						<dt class="text-muted-foreground">Not before</dt>
						<dd class="col-span-2">{detail.notBefore}</dd>
					{/if}
					{#if detail.notAfter}
						<dt class="text-muted-foreground">Not after</dt>
						<dd class="col-span-2">{detail.notAfter}</dd>
					{/if}
					{#if detail.createdAt}
						<dt class="text-muted-foreground">Created</dt>
						<dd class="col-span-2">{detail.createdAt}</dd>
					{/if}
				</dl>
				{#if detail.subjectAlternativeNames?.length}
					<h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
						Subject alternative names
					</h3>
					<ul class="space-y-1">
						{#each detail.subjectAlternativeNames as san (san)}
							<li class="rounded border border-border/60 px-3 py-1.5 font-mono text-xs">{san}</li>
						{/each}
					</ul>
				{/if}
				{#if detail.inUseBy?.length}
					<h3 class="mb-1.5 mt-4 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
						In use by
					</h3>
					<ul class="space-y-1">
						{#each detail.inUseBy as arn (arn)}
							<li class="break-all rounded border border-border/60 px-3 py-1.5 font-mono text-xs">
								{arn}
							</li>
						{/each}
					</ul>
				{/if}
			{/if}
		</div>
	</SheetContent>
</Sheet>
