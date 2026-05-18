<script lang="ts">
	import { toast } from 'svelte-sonner';
	import {
		putItem,
		attributeType,
		attributeToString,
		type AttributeValue,
		type Item,
		type TableDetail
	} from '$lib/api/dynamodb';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Badge } from '$lib/components/ui/badge';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';

	interface Props {
		open: boolean;
		detail: TableDetail | null;
		item: Item | null;
		onClose: () => void;
		onSaved: () => void;
	}

	let {
		open = $bindable(false),
		detail,
		item,
		onClose,
		onSaved
	}: Props = $props();

	type FieldType = 'S' | 'N' | 'BOOL' | 'NULL' | 'JSON';

	const FIELD_TYPE_LABELS: Record<FieldType, string> = {
		S: 'String',
		N: 'Number',
		BOOL: 'Boolean',
		NULL: 'Null',
		JSON: 'JSON'
	};

	const JSON_PLACEHOLDER = '{"L": [{"S": "a"}]}';

	interface Field {
		key: string;
		type: FieldType;
		value: string;
		isKey: boolean;
	}

	let fields = $state<Field[]>([]);
	let saving = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (open) {
			error = null;
			fields = buildFields(detail, item);
		}
	});

	function buildFields(d: TableDetail | null, existing: Item | null): Field[] {
		if (!d) return [];
		const result: Field[] = [];
		const keyNames = new Set(d.keySchema.map((k) => k.attributeName));

		for (const ks of d.keySchema) {
			const def = d.attributeDefinitions.find((a) => a.attributeName === ks.attributeName);
			const v = existing?.[ks.attributeName];
			result.push({
				key: ks.attributeName,
				type: (def?.attributeType as FieldType) ?? 'S',
				value: v ? attrToInput(v) : '',
				isKey: true
			});
		}

		if (existing) {
			for (const [k, v] of Object.entries(existing)) {
				if (keyNames.has(k)) continue;
				result.push({
					key: k,
					type: detectType(v),
					value: attrToInput(v),
					isKey: false
				});
			}
		}
		return result;
	}

	function detectType(v: AttributeValue): FieldType {
		const t = attributeType(v);
		if (t === 'S' || t === 'N') return t;
		if (t === 'BOOL') return 'BOOL';
		if (t === 'NULL') return 'NULL';
		return 'JSON';
	}

	function attrToInput(v: AttributeValue): string {
		const t = attributeType(v);
		if (t === 'S' && 'S' in v) return v.S;
		if (t === 'N' && 'N' in v) return v.N;
		if (t === 'BOOL' && 'BOOL' in v) return String(v.BOOL);
		if (t === 'NULL') return 'null';
		return attributeToString(v);
	}

	function fieldToAttribute(f: Field): AttributeValue | null {
		switch (f.type) {
			case 'S':
				return { S: f.value };
			case 'N':
				return { N: f.value };
			case 'BOOL':
				return { BOOL: f.value === 'true' };
			case 'NULL':
				return { NULL: true };
			case 'JSON': {
				try {
					return JSON.parse(f.value) as AttributeValue;
				} catch {
					throw new Error(`Field "${f.key}": invalid JSON`);
				}
			}
		}
	}

	function addField() {
		fields = [...fields, { key: '', type: 'S', value: '', isKey: false }];
	}

	function removeField(idx: number) {
		const f = fields[idx];
		if (f.isKey) return;
		fields = fields.filter((_, i) => i !== idx);
	}

	async function save() {
		if (!detail) return;
		error = null;
		const built: Item = {};
		try {
			for (const f of fields) {
				if (!f.key.trim()) {
					throw new Error('Attribute name required');
				}
				const v = fieldToAttribute(f);
				if (v) built[f.key] = v;
			}
		} catch (e) {
			error = e instanceof Error ? e.message : 'Invalid item';
			return;
		}

		saving = true;
		try {
			await putItem(detail.name, built);
			toast.success(item ? 'Item updated' : 'Item created');
			onSaved();
			onClose();
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Failed to save item';
			error = msg;
			toast.error(msg);
		} finally {
			saving = false;
		}
	}
</script>

<Sheet bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<SheetContent class="flex w-full flex-col gap-0 p-0 sm:max-w-lg">
		<SheetHeader class="border-b border-border p-4">
			<SheetTitle>{item ? 'Edit item' : 'New item'}</SheetTitle>
			<SheetDescription>
				{detail?.name ?? ''} — type-aware editor
			</SheetDescription>
		</SheetHeader>

		<div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4">
			{#each fields as f, i (i)}
				<div class="flex flex-col gap-1.5 rounded-md border border-border p-3">
					<div class="flex items-center gap-2">
						<Input
							bind:value={f.key}
							placeholder="attribute name"
							readonly={f.isKey}
							class="h-8 flex-1 font-mono text-xs"
							aria-label="Attribute name"
						/>
						<Select
							type="single"
							value={f.type}
							onValueChange={(v) => (f.type = v as FieldType)}
						>
							<SelectTrigger
								aria-label="Attribute type"
								size="sm"
								class="w-28 text-xs"
							>
								{FIELD_TYPE_LABELS[f.type]}
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="S" label="String">String</SelectItem>
								<SelectItem value="N" label="Number">Number</SelectItem>
								<SelectItem value="BOOL" label="Boolean">Boolean</SelectItem>
								<SelectItem value="NULL" label="Null">Null</SelectItem>
								<SelectItem value="JSON" label="JSON">JSON</SelectItem>
							</SelectContent>
						</Select>
						{#if f.isKey}
							<Badge variant="outline">key</Badge>
						{:else}
							<Button
								variant="ghost"
								size="icon-xs"
								aria-label="Remove attribute"
								onclick={() => removeField(i)}
							>
								<X class="size-3" />
							</Button>
						{/if}
					</div>
					{#if f.type === 'BOOL'}
						<Select type="single" bind:value={f.value}>
							<SelectTrigger
								aria-label="Boolean value"
								size="sm"
								class="w-28 text-xs"
							>
								{f.value || 'true'}
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="true" label="true">true</SelectItem>
								<SelectItem value="false" label="false">false</SelectItem>
							</SelectContent>
						</Select>
					{:else if f.type === 'NULL'}
						<span class="text-[11px] text-muted-foreground">null</span>
					{:else if f.type === 'JSON'}
						<Textarea
							bind:value={f.value}
							rows={3}
							class="font-mono text-xs"
							placeholder={JSON_PLACEHOLDER}
						/>
					{:else}
						<Input
							bind:value={f.value}
							class="h-8 font-mono text-xs"
							placeholder={f.type === 'N' ? '0' : 'value'}
						/>
					{/if}
				</div>
			{/each}

			<Button variant="outline" size="sm" onclick={addField}>
				<Plus class="size-3.5" />
				Add attribute
			</Button>

			{#if error}
				<p class="rounded-md bg-destructive/10 px-2 py-1 text-xs text-destructive">
					{error}
				</p>
			{/if}
		</div>

		<footer class="flex shrink-0 items-center justify-end gap-2 border-t border-border p-4">
			<Button variant="outline" onclick={onClose} disabled={saving}>Cancel</Button>
			<Button onclick={save} disabled={saving || fields.length === 0}>
				{#if saving}
					<Loader2 class="size-3.5 animate-spin" />
				{/if}
				Save
			</Button>
		</footer>
	</SheetContent>
</Sheet>
