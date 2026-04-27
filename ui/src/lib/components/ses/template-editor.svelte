<script lang="ts">
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle,
		SheetFooter
	} from '$lib/components/ui/sheet';
	import { Tabs, TabsContent, TabsList, TabsTrigger } from '$lib/components/ui/tabs';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
	import { Skeleton } from '$lib/components/ui/skeleton';
	import { toast } from 'svelte-sonner';
	import { getTemplate, createTemplate } from '$lib/api/ses';

	interface Props {
		open: boolean;
		onOpenChange: (open: boolean) => void;
		templateName: string | null;
		onSaved: () => void;
	}

	let { open = $bindable(), onOpenChange, templateName, onSaved }: Props = $props();

	let loading = $state(false);
	let saving = $state(false);
	let name = $state('');
	let subject = $state('');
	let html = $state('');
	let text = $state('');
	let active = $state('html');

	const isNew = $derived(templateName === null);

	$effect(() => {
		if (open) load();
	});

	async function load() {
		if (!templateName) {
			name = '';
			subject = '';
			html = '';
			text = '';
			return;
		}
		loading = true;
		try {
			const t = await getTemplate(templateName);
			name = t.name;
			subject = t.subject ?? '';
			html = t.html ?? '';
			text = t.text ?? '';
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to load template');
		} finally {
			loading = false;
		}
	}

	async function save() {
		if (!name.trim() || !subject.trim()) {
			toast.error('Name and subject are required.');
			return;
		}
		saving = true;
		try {
			await createTemplate({
				name: name.trim(),
				subject: subject.trim(),
				html: html || undefined,
				text: text || undefined
			});
			toast.success(isNew ? 'Template created.' : 'Template updated.');
			onSaved();
			onOpenChange(false);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to save template');
		} finally {
			saving = false;
		}
	}
</script>

<Sheet bind:open onOpenChange={(v) => onOpenChange(v)}>
	<SheetContent side="right" class="w-full max-w-2xl overflow-y-auto sm:max-w-2xl">
		<SheetHeader>
			<SheetTitle>{isNew ? 'New email template' : 'Edit email template'}</SheetTitle>
			<SheetDescription>
				Templates support handlebars-style variables substituted at send time via SendBulkEmail.
			</SheetDescription>
		</SheetHeader>

		<div class="flex flex-col gap-3 px-6 pb-6">
			{#if loading}
				<div class="flex flex-col gap-3">
					<Skeleton class="h-9 w-full" />
					<Skeleton class="h-9 w-full" />
					<Skeleton class="h-32 w-full" />
				</div>
			{:else}
				<div class="flex flex-col gap-1">
					<Label for="ses-tpl-name">Template name</Label>
					<Input
						id="ses-tpl-name"
						bind:value={name}
						placeholder="WelcomeEmail"
						readonly={!isNew}
						class="font-mono text-xs"
					/>
				</div>

				<div class="flex flex-col gap-1">
					<Label for="ses-tpl-subject">Subject</Label>
					<Input id="ses-tpl-subject" bind:value={subject} placeholder="Welcome, {{name}}!" />
				</div>

				<Tabs bind:value={active}>
					<TabsList variant="line">
						<TabsTrigger value="html">HTML</TabsTrigger>
						<TabsTrigger value="text">Text</TabsTrigger>
					</TabsList>
					<TabsContent value="html" class="mt-3">
						<Label for="ses-tpl-html" class="sr-only">HTML body</Label>
						<Textarea
							id="ses-tpl-html"
							bind:value={html}
							rows={12}
							class="font-mono text-xs"
							placeholder={'<h1>Hello {{name}}</h1>'}
						/>
					</TabsContent>
					<TabsContent value="text" class="mt-3">
						<Label for="ses-tpl-text" class="sr-only">Text body</Label>
						<Textarea
							id="ses-tpl-text"
							bind:value={text}
							rows={12}
							class="font-mono text-xs"
							placeholder={'Hello {{name}}'}
						/>
					</TabsContent>
				</Tabs>
			{/if}
		</div>

		<SheetFooter>
			<Button variant="outline" onclick={() => onOpenChange(false)}>Cancel</Button>
			<Button onclick={save} disabled={saving || loading || !name.trim() || !subject.trim()}>
				{saving ? 'Saving…' : isNew ? 'Create template' : 'Save changes'}
			</Button>
		</SheetFooter>
	</SheetContent>
</Sheet>
