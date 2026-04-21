<script lang="ts">
    import { onMount } from 'svelte';

    interface CertSummary {
        CertificateArn: string;
        DomainName: string;
        Status: string;
    }

    let certificates = $state<CertSummary[]>([]);
    let loading = $state(false);
    let error = $state<string | null>(null);
    let showRequest = $state(false);
    let newDomain = $state('');
    let newSans = $state('');
    let requesting = $state(false);
    let requestError = $state<string | null>(null);

    async function loadCertificates() {
        loading = true;
        error = null;
        try {
            const res = await fetch('http://localhost:4566/', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/x-amz-json-1.1',
                    'X-Amz-Target': 'CertificateManager.ListCertificates',
                    'Authorization': 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/acm/aws4_request, SignedHeaders=host, Signature=fake',
                },
                body: JSON.stringify({}),
            });
            const data = await res.json();
            certificates = data.CertificateSummaryList ?? [];
        } catch (e: any) {
            error = e.message;
        } finally {
            loading = false;
        }
    }

    async function requestCertificate() {
        if (!newDomain.trim()) return;
        requesting = true;
        requestError = null;
        try {
            const sans = newSans.split(',').map(s => s.trim()).filter(Boolean);
            const body: Record<string, unknown> = { DomainName: newDomain.trim(), ValidationMethod: 'DNS' };
            if (sans.length > 0) body['SubjectAlternativeNames'] = sans;
            const res = await fetch('http://localhost:4566/', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/x-amz-json-1.1',
                    'X-Amz-Target': 'CertificateManager.RequestCertificate',
                    'Authorization': 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/acm/aws4_request, SignedHeaders=host, Signature=fake',
                },
                body: JSON.stringify(body),
            });
            if (!res.ok) {
                const err = await res.json();
                throw new Error(err.message ?? res.statusText);
            }
            newDomain = '';
            newSans = '';
            showRequest = false;
            await loadCertificates();
        } catch (e: any) {
            requestError = e.message;
        } finally {
            requesting = false;
        }
    }

    async function deleteCertificate(arn: string) {
        if (!confirm('Delete certificate?')) return;
        await fetch('http://localhost:4566/', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/x-amz-json-1.1',
                'X-Amz-Target': 'CertificateManager.DeleteCertificate',
                'Authorization': 'AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/acm/aws4_request, SignedHeaders=host, Signature=fake',
            },
            body: JSON.stringify({ CertificateArn: arn }),
        });
        await loadCertificates();
    }

    onMount(loadCertificates);
</script>

<div class="p-6 max-w-5xl mx-auto">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold text-zinc-100">Certificate Manager (ACM)</h1>
            <p class="text-zinc-400 text-sm mt-1">Manage SSL/TLS certificates</p>
        </div>
        <button
            onclick={() => (showRequest = !showRequest)}
            class="px-4 py-2 bg-orange-600 hover:bg-orange-500 text-white rounded text-sm font-medium"
        >
            Request Certificate
        </button>
    </div>

    {#if showRequest}
        <div class="mb-6 p-4 bg-zinc-900 border border-zinc-700 rounded-lg">
            <h2 class="text-sm font-semibold text-zinc-200 mb-3">Request New Certificate</h2>
            <div class="space-y-3">
                <div>
                    <label class="block text-xs text-zinc-400 mb-1">Domain Name</label>
                    <input
                        type="text"
                        bind:value={newDomain}
                        placeholder="example.com"
                        class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100 focus:outline-none focus:border-orange-500"
                    />
                </div>
                <div>
                    <label class="block text-xs text-zinc-400 mb-1">Subject Alternative Names (comma-separated)</label>
                    <input
                        type="text"
                        bind:value={newSans}
                        placeholder="www.example.com, api.example.com"
                        class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-sm text-zinc-100 focus:outline-none focus:border-orange-500"
                    />
                </div>
                {#if requestError}
                    <p class="text-red-400 text-xs">{requestError}</p>
                {/if}
                <div class="flex gap-2">
                    <button
                        onclick={requestCertificate}
                        disabled={requesting || !newDomain.trim()}
                        class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 text-white rounded text-sm"
                    >
                        {requesting ? 'Requesting...' : 'Request'}
                    </button>
                    <button
                        onclick={() => (showRequest = false)}
                        class="px-4 py-2 bg-zinc-700 hover:bg-zinc-600 text-zinc-200 rounded text-sm"
                    >
                        Cancel
                    </button>
                </div>
            </div>
        </div>
    {/if}

    {#if loading}
        <p class="text-zinc-400 text-sm">Loading...</p>
    {:else if error}
        <p class="text-red-400 text-sm">{error}</p>
    {:else if certificates.length === 0}
        <div class="text-center py-16 text-zinc-500">
            <p class="text-lg">No certificates</p>
            <p class="text-sm mt-1">Request a certificate to get started.</p>
        </div>
    {:else}
        <div class="space-y-2">
            {#each certificates as cert}
                <div class="flex items-center justify-between p-4 bg-zinc-900 border border-zinc-800 rounded-lg">
                    <div class="min-w-0 flex-1">
                        <p class="text-sm font-medium text-zinc-100">{cert.DomainName}</p>
                        <p class="text-xs text-zinc-500 mt-0.5 font-mono truncate">{cert.CertificateArn}</p>
                    </div>
                    <div class="flex items-center gap-3 ml-4">
                        <span class="px-2 py-0.5 rounded text-xs font-medium bg-green-900 text-green-300">
                            {cert.Status}
                        </span>
                        <button
                            onclick={() => deleteCertificate(cert.CertificateArn)}
                            class="px-3 py-1.5 text-xs bg-red-900 hover:bg-red-800 text-red-200 rounded"
                        >
                            Delete
                        </button>
                    </div>
                </div>
            {/each}
        </div>
    {/if}
</div>
