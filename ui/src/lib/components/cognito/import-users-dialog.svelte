<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { adminCreateUser } from '$lib/api/cognito';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import { Select, SelectContent, SelectItem, SelectTrigger } from '$lib/components/ui/select';
	import Loader2 from '@lucide/svelte/icons/loader-2';
	import Upload from '@lucide/svelte/icons/upload';

	interface Props {
		open: boolean;
		poolId: string;
		onClose: () => void;
		onComplete: () => void;
	}

	let { open = $bindable(false), poolId, onClose, onComplete }: Props = $props();

	type Status = 'idle' | 'parsing' | 'importing' | 'done';

	let fileName = $state('');
	let rows = $state<Record<string, string>[]>([]);
	let columns = $state<string[]>([]);
	let usernameColumn = $state('');
	let suppressInvite = $state(true);
	let status = $state<Status>('idle');
	let processed = $state(0);
	let imported = $state(0);
	let errors = $state<{ row: number; username: string; error: string }[]>([]);

	$effect(() => {
		if (!open) {
			fileName = '';
			rows = [];
			columns = [];
			usernameColumn = '';
			suppressInvite = true;
			status = 'idle';
			processed = 0;
			imported = 0;
			errors = [];
		}
	});

	async function pickFile(e: Event) {
		const input = e.target as HTMLInputElement;
		const file = input.files?.[0];
		if (!file) return;
		fileName = file.name;
		status = 'parsing';
		try {
			const text = await file.text();
			parseCsv(text);
			status = 'idle';
		} catch (err) {
			toast.error(err instanceof Error ? err.message : 'CSV parse failed');
			status = 'idle';
		}
	}

	/// Minimal RFC-4180-ish CSV: comma-separated, double-quote escapes, no
	/// embedded newlines in quoted cells. Sufficient for Cognito-style
	/// import files which are flat key/value rows.
	function parseCsv(text: string) {
		const lines = text.split(/\r?\n/).filter((l) => l.length > 0);
		if (lines.length < 2) {
			throw new Error('CSV needs a header row and at least one data row');
		}
		const headers = splitRow(lines[0]);
		columns = headers;
		// Pick a sensible default username column.
		usernameColumn =
			headers.find((h) => h.toLowerCase() === 'cognito:username') ??
			headers.find((h) => h.toLowerCase() === 'username') ??
			headers.find((h) => h.toLowerCase() === 'email') ??
			headers[0];
		rows = lines.slice(1).map((line) => {
			const cells = splitRow(line);
			const r: Record<string, string> = {};
			headers.forEach((h, i) => {
				r[h] = cells[i] ?? '';
			});
			return r;
		});
	}

	function splitRow(line: string): string[] {
		const out: string[] = [];
		let cur = '';
		let inQuotes = false;
		for (let i = 0; i < line.length; i++) {
			const ch = line[i];
			if (inQuotes) {
				if (ch === '"') {
					if (line[i + 1] === '"') {
						cur += '"';
						i++;
					} else {
						inQuotes = false;
					}
				} else {
					cur += ch;
				}
			} else if (ch === '"') {
				inQuotes = true;
			} else if (ch === ',') {
				out.push(cur);
				cur = '';
			} else {
				cur += ch;
			}
		}
		out.push(cur);
		return out;
	}

	async function startImport() {
		if (!usernameColumn) {
			toast.error('Pick a username column');
			return;
		}
		status = 'importing';
		processed = 0;
		imported = 0;
		errors = [];
		for (let i = 0; i < rows.length; i++) {
			const r = rows[i];
			const username = (r[usernameColumn] ?? '').trim();
			processed = i + 1;
			if (!username) {
				errors.push({ row: i + 2, username: '', error: 'empty username' });
				continue;
			}
			const attrs: { name: string; value: string }[] = [];
			for (const [k, v] of Object.entries(r)) {
				if (k === usernameColumn) continue;
				if (k.startsWith('cognito:')) continue;
				const trimmed = (v ?? '').trim();
				if (trimmed) attrs.push({ name: k, value: trimmed });
			}
			try {
				await adminCreateUser({
					poolId,
					username,
					attributes: attrs.length > 0 ? attrs : undefined,
					messageAction: suppressInvite ? 'SUPPRESS' : undefined
				});
				imported++;
			} catch (e) {
				errors.push({
					row: i + 2,
					username,
					error: e instanceof Error ? e.message : String(e)
				});
			}
		}
		status = 'done';
		toast.success(`Imported ${imported} of ${rows.length}`);
		onComplete();
	}
</script>

<Dialog bind:open onOpenChange={(v: boolean) => !v && onClose()}>
	<DialogContent class="sm:max-w-2xl">
		<DialogHeader>
			<DialogTitle>Import users from CSV</DialogTitle>
			<DialogDescription>
				One row per user. The header row picks attribute names; each user is created via
				AdminCreateUser. Real AWS uses S3 + a presigned upload — awsim shortcuts that.
			</DialogDescription>
		</DialogHeader>

		<div class="flex flex-col gap-3">
			<label
				class="flex cursor-pointer items-center justify-center gap-2 rounded border border-dashed border-border px-6 py-8 text-sm text-muted-foreground hover:bg-muted/40"
			>
				<Upload class="size-4" />
				{fileName || 'Select CSV file...'}
				<input type="file" accept=".csv,text/csv" onchange={pickFile} class="sr-only" />
			</label>

			{#if columns.length > 0}
				<div class="grid gap-2 sm:grid-cols-2">
					<div class="space-y-1.5">
						<Label for="csv-username">Username column</Label>
						<Select type="single" bind:value={usernameColumn}>
							<SelectTrigger id="csv-username" class="w-full text-sm">
								{usernameColumn}
							</SelectTrigger>
							<SelectContent>
								{#each columns as c (c)}
									<SelectItem value={c} label={c}>{c}</SelectItem>
								{/each}
							</SelectContent>
						</Select>
					</div>
					<div class="flex items-end">
						<label class="flex items-center gap-2 text-xs text-muted-foreground">
							<input type="checkbox" bind:checked={suppressInvite} class="size-3.5" />
							Suppress invitation message
						</label>
					</div>
				</div>
				<p class="text-xs text-muted-foreground">
					{rows.length.toLocaleString()} rows ready · all other non-<code>cognito:</code> columns
					become user attributes.
				</p>
			{/if}

			{#if status === 'importing' || status === 'done'}
				<div class="space-y-1.5 rounded border border-border/60 px-3 py-2 text-xs">
					<div class="flex justify-between">
						<span>
							{status === 'importing' ? 'Importing' : 'Done'} — {processed} / {rows.length}
						</span>
						<span>
							✓ {imported.toLocaleString()}
							{#if errors.length > 0}
								· ✗ {errors.length.toLocaleString()}
							{/if}
						</span>
					</div>
					<div class="h-1.5 overflow-hidden rounded bg-muted">
						<div
							class="h-full bg-primary transition-all"
							style="width: {rows.length > 0 ? (processed / rows.length) * 100 : 0}%"
						></div>
					</div>
				</div>

				{#if errors.length > 0}
					<details class="rounded border border-destructive/40 px-3 py-2 text-xs">
						<summary class="cursor-pointer text-destructive">
							{errors.length} error{errors.length === 1 ? '' : 's'}
						</summary>
						<ul class="mt-2 space-y-1">
							{#each errors.slice(0, 50) as e (e.row)}
								<li class="font-mono text-[11px]">
									row {e.row}{e.username ? ` (${e.username})` : ''}: {e.error}
								</li>
							{/each}
							{#if errors.length > 50}
								<li class="text-muted-foreground">…and {errors.length - 50} more</li>
							{/if}
						</ul>
					</details>
				{/if}
			{/if}
		</div>

		<DialogFooter>
			<Button
				variant="outline"
				onclick={onClose}
				disabled={status === 'importing' || status === 'parsing'}
			>
				{status === 'done' ? 'Close' : 'Cancel'}
			</Button>
			<Button
				onclick={startImport}
				disabled={status !== 'idle' || rows.length === 0 || !usernameColumn}
			>
				{#if status === 'importing'}<Loader2 class="size-3.5 animate-spin" />{/if}
				Import {rows.length || ''}
			</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
