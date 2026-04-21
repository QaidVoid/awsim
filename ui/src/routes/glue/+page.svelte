<script lang="ts">
    import { onMount } from 'svelte';

    const BASE = 'http://localhost:4566';

    interface GlueDatabase {
        Name: string;
        Description?: string;
        CreateTime?: string;
    }

    interface Crawler {
        Name: string;
        Role: string;
        State: string;
        DatabaseName?: string;
    }

    interface GlueJob {
        Name: string;
        Role: string;
    }

    let databases = $state<GlueDatabase[]>([]);
    let crawlers = $state<Crawler[]>([]);
    let jobs = $state<GlueJob[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    async function glueRequest(target: string, body: Record<string, unknown> = {}) {
        const res = await fetch(BASE, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/x-amz-json-1.1',
                'X-Amz-Target': `AWSGlue.${target}`,
            },
            body: JSON.stringify(body),
        });
        if (!res.ok) throw new Error(await res.text());
        return res.json();
    }

    onMount(async () => {
        try {
            const [dbData, crawlerData, jobData] = await Promise.all([
                glueRequest('GetDatabases'),
                glueRequest('GetCrawlers'),
                glueRequest('GetJobs'),
            ]);
            databases = dbData.DatabaseList ?? [];
            crawlers = crawlerData.Crawlers ?? [];
            jobs = jobData.Jobs ?? [];
        } catch {
            error = 'Could not connect to AWSim. Is it running on port 4566?';
        } finally {
            loading = false;
        }
    });
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">Glue</h1>
            <p class="text-zinc-500 mt-1">Managed ETL service and Data Catalog.</p>
        </div>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else}
        <!-- Databases -->
        <h2 class="text-lg font-semibold mb-3">Databases ({databases.length})</h2>
        {#if databases.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-6 text-center mb-6">
                <p class="text-zinc-500">No Glue databases yet. Create one:</p>
                <code class="block mt-3 text-sm text-orange-400 font-mono">
                    aws --endpoint-url http://localhost:4566 glue create-database --database-input Name=mydb
                </code>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden mb-6">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Name</th>
                            <th class="px-4 py-3">Description</th>
                            <th class="px-4 py-3">Created</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each databases as db}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400">{db.Name}</td>
                                <td class="px-4 py-3 text-zinc-500">{db.Description ?? ''}</td>
                                <td class="px-4 py-3 text-zinc-500 text-xs">{db.CreateTime ?? ''}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}

        <!-- Crawlers -->
        <h2 class="text-lg font-semibold mb-3">Crawlers ({crawlers.length})</h2>
        {#if crawlers.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-6 text-center mb-6">
                <p class="text-zinc-500">No crawlers yet. Create one:</p>
                <code class="block mt-3 text-sm text-orange-400 font-mono">
                    aws --endpoint-url http://localhost:4566 glue create-crawler --name my-crawler --role arn:aws:iam::000000000000:role/glue-role --targets S3Targets=[&#123;Path=s3://my-bucket&#125;]
                </code>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden mb-6">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Name</th>
                            <th class="px-4 py-3">State</th>
                            <th class="px-4 py-3">Role</th>
                            <th class="px-4 py-3">Database</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each crawlers as crawler}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400">{crawler.Name}</td>
                                <td class="px-4 py-3 text-zinc-300">{crawler.State}</td>
                                <td class="px-4 py-3 text-zinc-500 text-xs truncate max-w-xs">{crawler.Role}</td>
                                <td class="px-4 py-3 text-zinc-500">{crawler.DatabaseName ?? ''}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}

        <!-- Jobs -->
        <h2 class="text-lg font-semibold mb-3">Jobs ({jobs.length})</h2>
        {#if jobs.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-6 text-center">
                <p class="text-zinc-500">No Glue jobs yet. Create one:</p>
                <code class="block mt-3 text-sm text-orange-400 font-mono">
                    aws --endpoint-url http://localhost:4566 glue create-job --name my-job --role arn:aws:iam::000000000000:role/glue-role --command ScriptLocation=s3://my-bucket/script.py
                </code>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Name</th>
                            <th class="px-4 py-3">Role</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each jobs as job}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400">{job.Name}</td>
                                <td class="px-4 py-3 text-zinc-500 text-xs truncate max-w-xs">{job.Role}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}
</div>
