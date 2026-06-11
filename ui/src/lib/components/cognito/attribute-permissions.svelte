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

	// Attributes eligible for write (immutable ones can never be granted write).
	let writable = $derived(schema.filter((a) => a.mutable));

	let readGranted = $derived(schema.filter((a) => read.includes(a.name)).length);
	let writeGranted = $derived(writable.filter((a) => write.includes(a.name)).length);
	let allRead = $derived(schema.length > 0 && readGranted === schema.length);
	let allWrite = $derived(writable.length > 0 && writeGranted === writable.length);
	// Attributes the client cannot read - the ones you'd need to grant after
	// adding a new custom attribute. Surfaced so it is never a silent gap.
	let ungranted = $derived(schema.filter((a) => !read.includes(a.name)).map((a) => a.name));

	function toggleAllRead() {
		read = allRead ? [] : schema.map((a) => a.name);
	}
	function toggleAllWrite() {
		write = allWrite ? [] : writable.map((a) => a.name);
	}
</script>

<div class="max-h-64 overflow-y-auto rounded border border-border">
	<table class="w-full text-sm">
		<thead
			class="sticky top-0 bg-muted/40 text-xs uppercase tracking-wide text-muted-foreground backdrop-blur"
		>
			<tr>
				<th class="px-3 py-2 text-left font-medium">Attribute</th>
				<th class="w-16 px-3 py-2 text-center font-medium">
					<div class="flex flex-col items-center gap-0.5">
						<span>Read</span>
						<input
							type="checkbox"
							class="size-3.5"
							checked={allRead}
							indeterminate={readGranted > 0 && !allRead}
							onchange={toggleAllRead}
							aria-label="Toggle read for all attributes"
							title="Read all / none"
						/>
					</div>
				</th>
				<th class="w-16 px-3 py-2 text-center font-medium">
					<div class="flex flex-col items-center gap-0.5">
						<span>Write</span>
						<input
							type="checkbox"
							class="size-3.5"
							checked={allWrite}
							indeterminate={writeGranted > 0 && !allWrite}
							onchange={toggleAllWrite}
							aria-label="Toggle write for all mutable attributes"
							title="Write all / none (mutable only)"
						/>
					</div>
				</th>
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
<p class="mt-1 text-xs text-muted-foreground">
	{readGranted}/{schema.length} readable, {writeGranted}/{writable.length} writable.
	{#if ungranted.length > 0}
		<span class="text-amber-600 dark:text-amber-500">
			Not readable: {ungranted.join(', ')}. Use the Read header to grant all.
		</span>
	{/if}
</p>
