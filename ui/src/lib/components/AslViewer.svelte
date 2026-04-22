<script lang="ts">
    let { definition } = $props<{ definition: string }>();

    interface StateNode {
        name: string;
        type: string;
        x: number;
        y: number;
        next?: string;
        isEnd: boolean;
        branches?: string[][];
    }

    let nodes = $derived(parseAsl(definition));
    let edges = $derived(computeEdges(nodes));
    let svgHeight = $derived(nodes.length * 80 + 60);

    function parseAsl(def: string): StateNode[] {
        try {
            const asl = JSON.parse(def);
            const states = asl.States || {};
            const startAt = asl.StartAt;
            const nodeMap: StateNode[] = [];

            let y = 40;
            let visited = new Set<string>();
            let queue = [startAt];

            while (queue.length > 0) {
                const name = queue.shift()!;
                if (visited.has(name) || !states[name]) continue;
                visited.add(name);

                const state = states[name];
                const node: StateNode = {
                    name,
                    type: state.Type,
                    x: 200,
                    y,
                    next: state.Next,
                    isEnd: state.End === true || state.Type === 'Succeed' || state.Type === 'Fail',
                };

                nodeMap.push(node);
                y += 80;

                if (state.Next && !visited.has(state.Next)) {
                    queue.push(state.Next);
                }

                if (state.Type === 'Choice' && state.Choices) {
                    for (const choice of state.Choices) {
                        if (choice.Next && !visited.has(choice.Next)) {
                            queue.push(choice.Next);
                        }
                    }
                    if (state.Default && !visited.has(state.Default)) {
                        queue.push(state.Default);
                    }
                }

                if (state.Type === 'Parallel' && state.Branches) {
                    node.branches = state.Branches.map((b: { States?: Record<string, unknown> }) => Object.keys(b.States || {}));
                }

                for (const catcher of (state.Catch || [])) {
                    if (catcher.Next && !visited.has(catcher.Next)) {
                        queue.push(catcher.Next);
                    }
                }
            }

            return nodeMap;
        } catch {
            return [];
        }
    }

    function computeEdges(nodes: StateNode[]): { from: StateNode; to: StateNode; label?: string }[] {
        const edges: { from: StateNode; to: StateNode; label?: string }[] = [];
        const nodeMap = new Map(nodes.map((n) => [n.name, n]));

        for (const node of nodes) {
            if (node.next) {
                const target = nodeMap.get(node.next);
                if (target) edges.push({ from: node, to: target });
            }
            if (node.type === 'Choice') {
                try {
                    const asl = JSON.parse(definition);
                    const state = asl.States[node.name];
                    for (const choice of state.Choices || []) {
                        const target = nodeMap.get(choice.Next);
                        if (target) edges.push({ from: node, to: target, label: 'choice' });
                    }
                    if (state.Default) {
                        const target = nodeMap.get(state.Default);
                        if (target) edges.push({ from: node, to: target, label: 'default' });
                    }
                } catch {
                    // ignore parse errors
                }
            }
        }
        return edges;
    }

    function stateColor(type: string): string {
        switch (type) {
            case 'Task': return '#f97316';
            case 'Choice': return '#a855f7';
            case 'Parallel': return '#3b82f6';
            case 'Wait': return '#eab308';
            case 'Pass': return '#6b7280';
            case 'Succeed': return '#22c55e';
            case 'Fail': return '#ef4444';
            case 'Map': return '#06b6d4';
            default: return '#6b7280';
        }
    }
</script>

{#if nodes.length > 0}
<svg width="420" height={svgHeight} class="bg-zinc-950 rounded-lg border border-zinc-800">
    <defs>
        <marker id="arrowhead" markerWidth="10" markerHeight="7" refX="10" refY="3.5" orient="auto">
            <polygon points="0 0, 10 3.5, 0 7" fill="#71717a" />
        </marker>
    </defs>

    <!-- Edges -->
    {#each edges as edge}
        <line
            x1={edge.from.x + 80} y1={edge.from.y + 20}
            x2={edge.to.x + 80} y2={edge.to.y - 4}
            stroke="#71717a" stroke-width="1.5"
            marker-end="url(#arrowhead)"
        />
    {/each}

    <!-- Nodes -->
    {#each nodes as node}
        <g>
            <rect
                x={node.x} y={node.y - 16}
                width="160" height="36"
                rx="6"
                fill="transparent"
                stroke={stateColor(node.type)}
                stroke-width="1.5"
            />
            <text x={node.x + 80} y={node.y + 4}
                text-anchor="middle" fill="#e4e4e7" font-size="12" font-family="monospace">
                {node.name}
            </text>
            <text x={node.x + 164} y={node.y + 4}
                fill={stateColor(node.type)} font-size="9" font-family="sans-serif">
                {node.type}
            </text>
        </g>
    {/each}
</svg>
{:else}
<div class="text-zinc-600 text-sm">No valid ASL definition</div>
{/if}
