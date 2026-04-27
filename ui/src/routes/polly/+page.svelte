<script lang="ts">
	import { ServicePage } from '$lib/components/service';
	import {
		Tabs,
		TabsList,
		TabsTrigger,
		TabsContent,
	} from '$lib/components/ui/tabs';
	import VoicesTab from '$lib/components/polly/voices-tab.svelte';
	import LexiconsTab from '$lib/components/polly/lexicons-tab.svelte';
	import SynthesizePanel from '$lib/components/polly/synthesize-panel.svelte';
	import TasksTab from '$lib/components/polly/tasks-tab.svelte';

	type TabKey = 'synth' | 'voices' | 'lexicons' | 'tasks';
	let activeTab = $state<TabKey>('synth');
</script>

<ServicePage
	title="Polly"
	description="Text-to-speech with managed voices, pronunciation lexicons, and async tasks."
>
	<Tabs bind:value={activeTab} class="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
		<TabsList variant="line" class="border-b border-border px-4">
			<TabsTrigger value="synth">Synthesize</TabsTrigger>
			<TabsTrigger value="voices">Voices</TabsTrigger>
			<TabsTrigger value="lexicons">Lexicons</TabsTrigger>
			<TabsTrigger value="tasks">Tasks</TabsTrigger>
		</TabsList>

		<div class="min-h-0 flex-1 overflow-y-auto">
			<TabsContent value="synth" class="m-0">
				<SynthesizePanel />
			</TabsContent>
			<TabsContent value="voices" class="m-0">
				<VoicesTab />
			</TabsContent>
			<TabsContent value="lexicons" class="m-0">
				<LexiconsTab />
			</TabsContent>
			<TabsContent value="tasks" class="m-0">
				<TasksTab />
			</TabsContent>
		</div>
	</Tabs>
</ServicePage>
