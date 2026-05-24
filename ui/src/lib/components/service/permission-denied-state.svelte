<script lang="ts">
	import { cn } from '$lib/utils';
	import ShieldOff from '@lucide/svelte/icons/shield-off';

	interface Props {
		/** AWS action the request needed (e.g. `s3:ListAllMyBuckets`). */
		action?: string;
		/** Optional ARN of the resource the action was attempted against. */
		resource?: string;
		/** Override the body description when the default doesn't fit. */
		description?: string | null;
		class?: string;
	}

	let { action, resource, description = null, class: className }: Props = $props();
</script>

<div
	class={cn(
		'flex flex-col items-center justify-center gap-3 rounded-md border border-dashed border-destructive/40 bg-destructive/5 px-6 py-12 text-center',
		className
	)}
>
	<ShieldOff class="size-7 text-destructive" />
	<div class="max-w-md space-y-1.5">
		<p class="text-sm font-medium">Permission denied</p>
		{#if description}
			<p class="text-xs text-muted-foreground">{description}</p>
		{:else}
			<p class="text-xs text-muted-foreground">
				Your IAM principal does not have permission to perform this action.
				Ask an admin to attach a policy that grants
				{#if action}
					<code class="rounded bg-muted/60 px-1 py-0.5 font-mono text-[11px]">{action}</code>
				{:else}
					the required action
				{/if}
				{#if resource}
					on
					<code class="rounded bg-muted/60 px-1 py-0.5 font-mono text-[11px]">{resource}</code>
				{/if}.
			</p>
		{/if}
	</div>
</div>
