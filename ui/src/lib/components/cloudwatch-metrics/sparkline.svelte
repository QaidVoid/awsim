<script lang="ts">
	/**
	 * Pure-SVG sparkline.
	 *
	 * Renders `data` as a polyline scaled to fit the configured `width` x
	 * `height`. Optionally fills the area under the line. No external
	 * chart library — flat SVG so it stays light and theme-friendly.
	 */
	import { cn } from '$lib/utils';

	interface Props {
		data: number[];
		width?: number;
		height?: number;
		stroke?: string;
		fill?: string;
		showArea?: boolean;
		class?: string;
		ariaLabel?: string;
	}

	let {
		data,
		width = 120,
		height = 28,
		stroke = 'currentColor',
		fill,
		showArea = true,
		class: className,
		ariaLabel,
	}: Props = $props();

	const padding = 1;

	const layout = $derived.by(() => {
		const pts = data;
		if (pts.length === 0) {
			return { d: '', area: '', min: 0, max: 0 };
		}
		const min = Math.min(...pts);
		const max = Math.max(...pts);
		const range = max - min || 1;
		const innerW = width - padding * 2;
		const innerH = height - padding * 2;
		const stepX = pts.length > 1 ? innerW / (pts.length - 1) : 0;
		const coords = pts.map((v, i) => {
			const x = padding + i * stepX;
			const y = padding + innerH - ((v - min) / range) * innerH;
			return [x, y] as const;
		});
		const d = coords
			.map((c, i) => `${i === 0 ? 'M' : 'L'}${c[0].toFixed(2)},${c[1].toFixed(2)}`)
			.join(' ');
		const area =
			coords.length > 1
				? `${d} L${coords[coords.length - 1][0].toFixed(2)},${(height - padding).toFixed(2)} L${coords[0][0].toFixed(2)},${(height - padding).toFixed(2)} Z`
				: '';
		return { d, area, min, max };
	});

	const fillColor = $derived(fill ?? 'currentColor');
</script>

<svg
	{width}
	{height}
	viewBox={`0 0 ${width} ${height}`}
	role="img"
	aria-label={ariaLabel ?? 'sparkline'}
	class={cn('overflow-visible', className)}
>
	{#if showArea && layout.area}
		<path d={layout.area} fill={fillColor} fill-opacity="0.12" />
	{/if}
	{#if layout.d}
		<path d={layout.d} fill="none" stroke={stroke} stroke-width="1.25" stroke-linejoin="round" stroke-linecap="round" />
	{/if}
</svg>
