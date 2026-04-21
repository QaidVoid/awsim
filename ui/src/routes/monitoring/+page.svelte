<script lang="ts">
    import { onMount } from 'svelte';

    const ENDPOINT = 'http://localhost:4566';

    interface MetricAlarm {
        alarmName: string;
        namespace: string;
        metricName: string;
        stateValue: string;
        threshold: number;
        comparisonOperator: string;
        period: number;
    }

    interface Metric {
        namespace: string;
        metricName: string;
    }

    let alarms = $state<MetricAlarm[]>([]);
    let metrics = $state<Metric[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);
    let activeTab = $state<'alarms' | 'metrics'>('alarms');

    async function fetchAlarms() {
        const res = await fetch(`${ENDPOINT}/`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
            body: 'Action=DescribeAlarms&Version=2010-08-01',
        });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const text = await res.text();
        const parser = new DOMParser();
        const doc = parser.parseFromString(text, 'application/xml');
        const alarmEls = doc.querySelectorAll('member');
        alarms = Array.from(alarmEls).map((el) => ({
            alarmName: el.querySelector('AlarmName')?.textContent ?? '',
            namespace: el.querySelector('Namespace')?.textContent ?? '',
            metricName: el.querySelector('MetricName')?.textContent ?? '',
            stateValue: el.querySelector('StateValue')?.textContent ?? '',
            threshold: parseFloat(el.querySelector('Threshold')?.textContent ?? '0'),
            comparisonOperator: el.querySelector('ComparisonOperator')?.textContent ?? '',
            period: parseInt(el.querySelector('Period')?.textContent ?? '60', 10),
        })).filter(a => a.alarmName !== '');
    }

    async function fetchMetrics() {
        const res = await fetch(`${ENDPOINT}/`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
            body: 'Action=ListMetrics&Version=2010-08-01',
        });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const text = await res.text();
        const parser = new DOMParser();
        const doc = parser.parseFromString(text, 'application/xml');
        const metricEls = doc.querySelectorAll('member');
        metrics = Array.from(metricEls).map((el) => ({
            namespace: el.querySelector('Namespace')?.textContent ?? '',
            metricName: el.querySelector('MetricName')?.textContent ?? '',
        })).filter(m => m.metricName !== '');
    }

    onMount(async () => {
        try {
            await Promise.all([fetchAlarms(), fetchMetrics()]);
        } catch {
            error = 'Could not connect to AWSim. Is it running on port 4566?';
        } finally {
            loading = false;
        }
    });

    function stateColor(state: string) {
        switch (state) {
            case 'OK': return 'text-green-400';
            case 'ALARM': return 'text-red-400';
            default: return 'text-zinc-400';
        }
    }
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">CloudWatch Metrics</h1>
            <p class="text-zinc-500 mt-1">Monitor metrics, manage alarms and dashboards.</p>
        </div>
    </div>

    <!-- Tabs -->
    <div class="flex gap-2 mb-6 border-b border-zinc-800">
        <button
            class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'alarms' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-500 hover:text-zinc-300'}"
            onclick={() => activeTab = 'alarms'}
        >
            Alarms ({alarms.length})
        </button>
        <button
            class="px-4 py-2 text-sm font-medium transition-colors {activeTab === 'metrics' ? 'text-orange-400 border-b-2 border-orange-400' : 'text-zinc-500 hover:text-zinc-300'}"
            onclick={() => activeTab = 'metrics'}
        >
            Metrics ({metrics.length})
        </button>
    </div>

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400">{error}</div>
    {:else if activeTab === 'alarms'}
        {#if alarms.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No alarms yet. Create one using the AWS CLI:</p>
                <code class="block mt-3 text-sm text-orange-400 font-mono">
                    aws --endpoint-url http://localhost:4566 cloudwatch put-metric-alarm --alarm-name my-alarm --metric-name CPUUtilization --namespace AWS/EC2 --statistic Average --period 60 --evaluation-periods 1 --threshold 80 --comparison-operator GreaterThanThreshold
                </code>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Alarm Name</th>
                            <th class="px-4 py-3">Namespace / Metric</th>
                            <th class="px-4 py-3">State</th>
                            <th class="px-4 py-3">Threshold</th>
                            <th class="px-4 py-3">Period</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each alarms as alarm}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 font-mono text-orange-400">{alarm.alarmName}</td>
                                <td class="px-4 py-3 text-zinc-400">
                                    <span class="text-zinc-300">{alarm.namespace}</span>
                                    <span class="text-zinc-600"> / </span>
                                    {alarm.metricName}
                                </td>
                                <td class="px-4 py-3 font-medium {stateColor(alarm.stateValue)}">{alarm.stateValue}</td>
                                <td class="px-4 py-3 text-zinc-400">{alarm.comparisonOperator} {alarm.threshold}</td>
                                <td class="px-4 py-3 text-zinc-400">{alarm.period}s</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {:else}
        {#if metrics.length === 0}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
                <p class="text-zinc-500">No metrics yet. Push some using the AWS CLI:</p>
                <code class="block mt-3 text-sm text-orange-400 font-mono">
                    aws --endpoint-url http://localhost:4566 cloudwatch put-metric-data --namespace MyApp --metric-name RequestCount --value 42 --unit Count
                </code>
            </div>
        {:else}
            <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                <table class="w-full text-sm">
                    <thead>
                        <tr class="border-b border-zinc-800 text-left text-zinc-500">
                            <th class="px-4 py-3">Namespace</th>
                            <th class="px-4 py-3">Metric Name</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each metrics as metric}
                            <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                                <td class="px-4 py-3 text-zinc-300 font-mono">{metric.namespace}</td>
                                <td class="px-4 py-3 text-orange-400 font-mono">{metric.metricName}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}
    {/if}
</div>
