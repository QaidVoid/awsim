<script lang="ts" module>
	import type { SchemaAttribute } from '$lib/api/cognito';

	/** AWS default permission set: every attribute is readable, every
	 * mutable attribute is writable. Used to seed the matrix when a
	 * client has no custom set configured yet. */
	export function defaultClientPerms(schema: SchemaAttribute[]): {
		read: string[];
		write: string[];
	} {
		return {
			read: schema.map((a) => a.name),
			write: schema.filter((a) => a.mutable).map((a) => a.name)
		};
	}
</script>

<script lang="ts">
	interface Props {
		schema: SchemaAttribute[];
		read: string[];
		write: string[];
	}

	let { schema, read = $bindable(), write = $bindable() }: Props = $props();

	function toggle(list: string[], name: string): string[] {
		return list.includes(name) ? list.filter((x) => x !== name) : [...list, name];
	}
</script>

<div class="max-h-64 overflow-y-auto rounded border border-border">
	<table class="w-full text-sm">
		<thead
			class="sticky top-0 bg-muted/40 text-xs uppercase tracking-wide text-muted-foreground backdrop-blur"
		>
			<tr>
				<th class="px-3 py-2 text-left font-medium">Attribute</th>
				<th class="w-16 px-3 py-2 text-center font-medium">Read</th>
				<th class="w-16 px-3 py-2 text-center font-medium">Write</th>
			</tr>
		</thead>
		<tbody>
			{#each schema as a (a.name)}
				<tr class="border-t border-border">
					<td class="px-3 py-1.5 font-mono text-xs">{a.name}</td>
					<td class="px-3 py-1.5 text-center">
						<input
							type="checkbox"
							class="size-3.5"
							checked={read.includes(a.name)}
							onchange={() => (read = toggle(read, a.name))}
							aria-label={`Read ${a.name}`}
						/>
					</td>
					<td class="px-3 py-1.5 text-center">
						<input
							type="checkbox"
							class="size-3.5 disabled:opacity-40"
							checked={a.mutable && write.includes(a.name)}
							disabled={!a.mutable}
							onchange={() => (write = toggle(write, a.name))}
							aria-label={`Write ${a.name}`}
							title={a.mutable ? undefined : 'Immutable attribute cannot be granted write'}
						/>
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>
