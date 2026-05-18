<script lang="ts">
	import { onMount } from 'svelte';
	import { ResourceConsole, EmptyState } from '$lib/components/service';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import {
		Dialog,
		DialogContent,
		DialogHeader,
		DialogTitle,
		DialogDescription,
		DialogFooter,
	} from '$lib/components/ui/dialog';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import InfoIcon from '@lucide/svelte/icons/info';
	import ContainerIcon from '@lucide/svelte/icons/container';
	import { toast } from 'svelte-sonner';
	import {
		describeRepositories,
		deleteRepository,
		type Repository,
		type Image,
	} from '$lib/api/ecr';
	import RepositoryList from '$lib/components/ecr/repository-list.svelte';
	import ImagesTab from '$lib/components/ecr/images-tab.svelte';
	import CreateRepoDialog from '$lib/components/ecr/create-repo-dialog.svelte';
	import RepositoryDetailSheet from '$lib/components/ecr/repository-detail-sheet.svelte';
	import ImageDetailSheet from '$lib/components/ecr/image-detail-sheet.svelte';

	let repositories = $state<Repository[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	let selectedName = $state<string | null>(null);
	let createOpen = $state(false);
	let detailOpen = $state(false);
	let imageOpen = $state(false);
	let selectedImage = $state<Image | null>(null);
	let confirmDelete = $state<{ name: string } | null>(null);
	let imagesRefreshKey = $state(0);

	let selectedRepo = $derived(
		repositories.find((r) => r.repositoryName === selectedName) ?? null
	);

	async function loadRepos() {
		loading = true;
		error = null;
		try {
			repositories = await describeRepositories();
			if (selectedName && !repositories.some((r) => r.repositoryName === selectedName)) {
				selectedName = null;
			}
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load repositories';
		} finally {
			loading = false;
		}
	}

	function handleSelect(name: string) {
		selectedName = name;
	}

	function handleImageSelect(img: Image) {
		selectedImage = img;
		imageOpen = true;
	}

	async function handleDelete() {
		if (!confirmDelete) return;
		const { name } = confirmDelete;
		confirmDelete = null;
		try {
			await deleteRepository(name);
			toast.success(`Repository ${name} deleted.`);
			if (selectedName === name) selectedName = null;
			await loadRepos();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : 'Failed to delete repository');
		}
	}

	function bumpImages() {
		imagesRefreshKey += 1;
	}

	onMount(loadRepos);
</script>

<ResourceConsole
	title="ECR"
	description="Elastic Container Registry. Store, version, and pull OCI / Docker images."
	listWidth="300px"
	listError={error}
	onListRetry={loadRepos}
	listLoading={loading}
	listIsEmpty={repositories.length === 0}
	listSkeletonRows={6}
	hasSelection={!!selectedRepo}
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={loadRepos} disabled={loading}>
			<RefreshCwIcon class={loading ? 'animate-spin' : ''} />
			Refresh
		</Button>
		<Button size="sm" onclick={() => (createOpen = true)}>
			<PlusIcon />
			New repository
		</Button>
	{/snippet}

	{#snippet listEmpty()}
		<EmptyState
			icon={ContainerIcon}
			title="No ECR repositories"
			description="Create a repository to push container images and use them from ECS, EKS, or Lambda."
		>
			{#snippet action()}
				<Button onclick={() => (createOpen = true)}>
					<PlusIcon />
					Create repository
				</Button>
			{/snippet}
		</EmptyState>
	{/snippet}

	{#snippet list()}
		<RepositoryList
			{repositories}
			{selectedName}
			onSelect={handleSelect}
			onCreate={() => (createOpen = true)}
		/>
	{/snippet}

	{#snippet empty()}
		<div class="flex h-full items-center justify-center text-sm text-muted-foreground">
			Select a repository to view its images.
		</div>
	{/snippet}

	{#snippet detailHeader()}
		{#if selectedRepo}
			<header
				class="flex items-center justify-between gap-3 border-b border-border px-5 py-3"
			>
				<div class="min-w-0">
					<div class="flex items-center gap-2">
						<h2 class="truncate font-mono text-sm font-medium">
							{selectedRepo.repositoryName}
						</h2>
						{#if selectedRepo.imageTagMutability === 'IMMUTABLE'}
							<Badge variant="outline" class="h-4 px-1.5 text-[10px]">IMMUTABLE</Badge>
						{/if}
					</div>
					<p class="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">
						{selectedRepo.repositoryUri}
					</p>
				</div>
				<div class="flex shrink-0 items-center gap-2">
					<Button size="sm" variant="outline" onclick={() => (detailOpen = true)}>
						<InfoIcon />
						Details
					</Button>
					<Button
						size="sm"
						variant="destructive"
						onclick={() => (confirmDelete = { name: selectedRepo!.repositoryName })}
					>
						<Trash2Icon />
						Delete
					</Button>
				</div>
			</header>
		{/if}
	{/snippet}

	{#if selectedRepo}
		<div class="min-h-0 flex-1 overflow-hidden">
			<ImagesTab
				repositoryName={selectedRepo.repositoryName}
				onSelect={handleImageSelect}
				refreshKey={imagesRefreshKey}
			/>
		</div>
	{/if}
</ResourceConsole>

<CreateRepoDialog
	open={createOpen}
	onOpenChange={(o) => (createOpen = o)}
	onCreated={(name) => {
		selectedName = name;
		void loadRepos();
	}}
/>

<RepositoryDetailSheet
	repo={selectedRepo}
	open={detailOpen}
	onOpenChange={(o) => (detailOpen = o)}
/>

<ImageDetailSheet
	image={selectedImage}
	open={imageOpen}
	onOpenChange={(o) => (imageOpen = o)}
	onDeleted={bumpImages}
/>

<Dialog
	open={confirmDelete !== null}
	onOpenChange={(o) => {
		if (!o) confirmDelete = null;
	}}
>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Delete repository?</DialogTitle>
			<DialogDescription>
				This permanently removes <span class="font-mono">{confirmDelete?.name}</span> and all
				of its images.
			</DialogDescription>
		</DialogHeader>
		<DialogFooter>
			<Button variant="outline" onclick={() => (confirmDelete = null)}>Cancel</Button>
			<Button variant="destructive" onclick={handleDelete}>Delete</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
