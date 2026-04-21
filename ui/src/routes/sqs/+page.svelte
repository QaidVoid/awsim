<script lang="ts">
    import { onMount } from 'svelte';
    import {
        listQueues,
        createQueue,
        deleteQueue,
        sendMessage,
        receiveMessages,
        deleteMessage,
        purgeQueue,
        getQueueAttributes,
        type SqsQueue,
        type SqsMessage,
        type SqsQueueAttributes,
    } from '$lib/aws';

    let queues = $state<SqsQueue[]>([]);
    let loading = $state(true);
    let error = $state<string | null>(null);

    let showCreateForm = $state(false);
    let newQueueName = $state('');
    let newQueueFifo = $state(false);
    let creating = $state(false);
    let createError = $state<string | null>(null);

    let selectedQueue = $state<string | null>(null);
    let queueAttrs = $state<SqsQueueAttributes | null>(null);
    let attrsLoading = $state(false);

    let messageBody = $state('');
    let sending = $state(false);
    let sendError = $state<string | null>(null);
    let sendSuccess = $state(false);

    let receivedMessages = $state<SqsMessage[]>([]);
    let receiving = $state(false);
    let receiveError = $state<string | null>(null);

    let confirmDelete = $state<string | null>(null);
    let confirmPurge = $state<string | null>(null);

    let queueAttrsMap = $state<Record<string, SqsQueueAttributes>>({});

    async function loadQueues() {
        loading = true;
        error = null;
        try {
            const data = await listQueues();
            queues = data.queues;
            // Load attributes in background
            for (const q of data.queues) {
                getQueueAttributes(q.url).then((attrs) => {
                    queueAttrsMap = { ...queueAttrsMap, [q.url]: attrs };
                }).catch(() => {});
            }
        } catch {
            error = 'Could not connect to AWSim. Is it running on port 4566?';
        } finally {
            loading = false;
        }
    }

    async function handleCreateQueue() {
        if (!newQueueName.trim()) return;
        creating = true;
        createError = null;
        try {
            await createQueue(newQueueName.trim(), newQueueFifo);
            newQueueName = '';
            newQueueFifo = false;
            showCreateForm = false;
            await loadQueues();
        } catch (e) {
            createError = e instanceof Error ? e.message : 'Failed to create queue';
        } finally {
            creating = false;
        }
    }

    async function handleDeleteQueue(url: string) {
        try {
            await deleteQueue(url);
            confirmDelete = null;
            if (selectedQueue === url) {
                selectedQueue = null;
                queueAttrs = null;
                receivedMessages = [];
            }
            await loadQueues();
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to delete queue';
        }
    }

    async function selectQueue(url: string) {
        selectedQueue = url;
        queueAttrs = null;
        receivedMessages = [];
        receiveError = null;
        sendError = null;
        sendSuccess = false;
        attrsLoading = true;
        try {
            queueAttrs = await getQueueAttributes(url);
        } catch {
            // attrs not critical
        } finally {
            attrsLoading = false;
        }
    }

    async function handleSendMessage() {
        if (!selectedQueue || !messageBody.trim()) return;
        sending = true;
        sendError = null;
        sendSuccess = false;
        try {
            await sendMessage(selectedQueue, messageBody.trim());
            messageBody = '';
            sendSuccess = true;
            setTimeout(() => sendSuccess = false, 2500);
            // Refresh attrs
            getQueueAttributes(selectedQueue).then((a) => { queueAttrs = a; queueAttrsMap = { ...queueAttrsMap, [selectedQueue!]: a }; }).catch(() => {});
        } catch (e) {
            sendError = e instanceof Error ? e.message : 'Failed to send message';
        } finally {
            sending = false;
        }
    }

    async function handleReceiveMessages() {
        if (!selectedQueue) return;
        receiving = true;
        receiveError = null;
        try {
            const res = await receiveMessages(selectedQueue, 10);
            receivedMessages = res.messages;
            if (res.messages.length === 0) {
                receiveError = 'No messages available in queue.';
            }
        } catch (e) {
            receiveError = e instanceof Error ? e.message : 'Failed to receive messages';
        } finally {
            receiving = false;
        }
    }

    async function handleDeleteMessage(receiptHandle: string) {
        if (!selectedQueue) return;
        try {
            await deleteMessage(selectedQueue, receiptHandle);
            receivedMessages = receivedMessages.filter((m) => m.receiptHandle !== receiptHandle);
            getQueueAttributes(selectedQueue).then((a) => { queueAttrs = a; }).catch(() => {});
        } catch (e) {
            receiveError = e instanceof Error ? e.message : 'Failed to delete message';
        }
    }

    async function handlePurgeQueue(url: string) {
        try {
            await purgeQueue(url);
            confirmPurge = null;
            receivedMessages = [];
            if (selectedQueue === url) {
                getQueueAttributes(url).then((a) => { queueAttrs = a; }).catch(() => {});
            }
            getQueueAttributes(url).then((attrs) => {
                queueAttrsMap = { ...queueAttrsMap, [url]: attrs };
            }).catch(() => {});
        } catch (e) {
            error = e instanceof Error ? e.message : 'Failed to purge queue';
        }
    }

    function selectedQueueObj(): SqsQueue | undefined {
        return queues.find((q) => q.url === selectedQueue);
    }

    onMount(loadQueues);
</script>

<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-bold">SQS — Queues</h1>
            <p class="text-zinc-500 mt-1">Simple Queue Service. Send, receive, and manage messages.</p>
        </div>
        <div class="flex items-center gap-3">
            <span class="text-sm text-zinc-500">{queues.length} queue{queues.length !== 1 ? 's' : ''}</span>
            <button
                onclick={() => { showCreateForm = !showCreateForm; createError = null; }}
                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium transition-colors"
            >
                Create Queue
            </button>
        </div>
    </div>

    {#if showCreateForm}
        <div class="bg-zinc-900 border border-zinc-700 rounded-lg p-4 mb-4">
            <h3 class="font-semibold mb-3">Create Queue</h3>
            {#if createError}
                <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{createError}</div>
            {/if}
            <input
                type="text"
                bind:value={newQueueName}
                onkeydown={(e) => e.key === 'Enter' && handleCreateQueue()}
                class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm focus:outline-none focus:border-orange-500"
                placeholder="my-queue-name"
            />
            <label class="flex items-center gap-2 mt-3 text-sm text-zinc-400 cursor-pointer select-none">
                <input type="checkbox" bind:checked={newQueueFifo} class="accent-orange-500" />
                FIFO Queue (appends .fifo suffix)
            </label>
            <div class="flex gap-2 mt-3">
                <button
                    onclick={handleCreateQueue}
                    disabled={creating || !newQueueName.trim()}
                    class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                >
                    {creating ? 'Creating...' : 'Create'}
                </button>
                <button
                    onclick={() => { showCreateForm = false; createError = null; newQueueName = ''; }}
                    class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 rounded text-sm transition-colors"
                >
                    Cancel
                </button>
            </div>
        </div>
    {/if}

    {#if error}
        <div class="bg-red-900/20 border border-red-800 rounded-lg p-4 text-red-400 mb-4">{error}</div>
    {/if}

    {#if loading}
        <div class="text-zinc-500">Loading...</div>
    {:else if queues.length === 0 && !showCreateForm}
        <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center">
            <p class="text-zinc-500">No queues yet.</p>
            <button
                onclick={() => showCreateForm = true}
                class="mt-3 px-3 py-1.5 bg-orange-600 hover:bg-orange-500 rounded text-sm font-medium"
            >
                Create your first queue
            </button>
        </div>
    {:else}
        <div class="flex gap-4">
            <!-- Queue list -->
            <div class="w-72 shrink-0">
                <div class="bg-zinc-900 rounded-lg border border-zinc-800 overflow-hidden">
                    {#each queues as queue}
                        <div
                            class="border-b border-zinc-800/50 last:border-0 {selectedQueue === queue.url ? 'bg-zinc-800' : 'hover:bg-zinc-800/40'} cursor-pointer transition-colors"
                        >
                            <div class="px-4 py-3 flex items-start justify-between gap-2">
                                <button class="flex-1 text-left min-w-0" onclick={() => selectQueue(queue.url)}>
                                    <div class="font-mono text-orange-400 text-sm truncate">{queue.name}</div>
                                    {#if queueAttrsMap[queue.url]}
                                        <div class="text-xs text-zinc-500 mt-0.5">
                                            {queueAttrsMap[queue.url].approximateNumberOfMessages} msg{queueAttrsMap[queue.url].approximateNumberOfMessages !== 1 ? 's' : ''}
                                            {#if queueAttrsMap[queue.url].isFifo}
                                                <span class="ml-1 text-orange-500/70">FIFO</span>
                                            {/if}
                                        </div>
                                    {/if}
                                </button>
                                <div class="flex flex-col gap-1 shrink-0">
                                    {#if confirmPurge === queue.url}
                                        <div class="flex gap-1">
                                            <button
                                                onclick={() => handlePurgeQueue(queue.url)}
                                                class="px-2 py-0.5 bg-yellow-700 hover:bg-yellow-600 rounded text-xs"
                                            >
                                                Purge
                                            </button>
                                            <button
                                                onclick={() => confirmPurge = null}
                                                class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs"
                                            >
                                                No
                                            </button>
                                        </div>
                                    {:else}
                                        <button
                                            onclick={(e) => { e.stopPropagation(); confirmPurge = queue.url; confirmDelete = null; }}
                                            class="px-2 py-0.5 text-yellow-500 hover:text-yellow-400 hover:bg-yellow-900/30 rounded text-xs transition-colors"
                                        >
                                            Purge
                                        </button>
                                    {/if}
                                    {#if confirmDelete === queue.url}
                                        <div class="flex gap-1">
                                            <button
                                                onclick={() => handleDeleteQueue(queue.url)}
                                                class="px-2 py-0.5 bg-red-700 hover:bg-red-600 rounded text-xs"
                                            >
                                                Del
                                            </button>
                                            <button
                                                onclick={() => confirmDelete = null}
                                                class="px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 rounded text-xs"
                                            >
                                                No
                                            </button>
                                        </div>
                                    {:else}
                                        <button
                                            onclick={(e) => { e.stopPropagation(); confirmDelete = queue.url; confirmPurge = null; }}
                                            class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors"
                                        >
                                            Delete
                                        </button>
                                    {/if}
                                </div>
                            </div>
                        </div>
                    {/each}
                </div>
            </div>

            <!-- Queue detail panel -->
            <div class="flex-1 min-w-0 flex flex-col gap-4">
                {#if selectedQueue}
                    <!-- Queue info -->
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
                        <h3 class="font-semibold mb-3 text-zinc-200">{selectedQueueObj()?.name}</h3>
                        {#if attrsLoading}
                            <div class="text-zinc-500 text-sm">Loading attributes...</div>
                        {:else if queueAttrs}
                            <div class="grid grid-cols-2 gap-x-6 gap-y-1 text-sm">
                                <div class="text-zinc-500">Messages available</div>
                                <div class="text-zinc-200">{queueAttrs.approximateNumberOfMessages}</div>
                                <div class="text-zinc-500">Messages in flight</div>
                                <div class="text-zinc-200">{queueAttrs.approximateNumberOfMessagesNotVisible}</div>
                                <div class="text-zinc-500">Visibility timeout</div>
                                <div class="text-zinc-200">{queueAttrs.visibilityTimeout}s</div>
                                <div class="text-zinc-500">Retention period</div>
                                <div class="text-zinc-200">{Math.round(queueAttrs.messageRetentionPeriod / 86400)}d</div>
                                <div class="text-zinc-500">Type</div>
                                <div class="text-zinc-200">{queueAttrs.isFifo ? 'FIFO' : 'Standard'}</div>
                            </div>
                        {/if}
                        <div class="mt-3 text-xs text-zinc-600 break-all font-mono">{selectedQueue}</div>
                    </div>

                    <!-- Send message -->
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
                        <h3 class="font-semibold mb-3 text-zinc-200">Send Message</h3>
                        {#if sendError}
                            <div class="bg-red-900/20 border border-red-800 rounded p-2 text-red-400 text-sm mb-3">{sendError}</div>
                        {/if}
                        {#if sendSuccess}
                            <div class="bg-green-900/20 border border-green-800 rounded p-2 text-green-400 text-sm mb-3">Message sent successfully.</div>
                        {/if}
                        <textarea
                            bind:value={messageBody}
                            rows={4}
                            class="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-orange-500 resize-y"
                            placeholder="message body (plain text or JSON)"
                        ></textarea>
                        <div class="mt-3">
                            <button
                                onclick={handleSendMessage}
                                disabled={sending || !messageBody.trim()}
                                class="px-3 py-1.5 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors"
                            >
                                {sending ? 'Sending...' : 'Send Message'}
                            </button>
                        </div>
                    </div>

                    <!-- Receive messages -->
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-4">
                        <div class="flex items-center justify-between mb-3">
                            <h3 class="font-semibold text-zinc-200">Receive Messages</h3>
                            <button
                                onclick={handleReceiveMessages}
                                disabled={receiving}
                                class="px-3 py-1.5 bg-zinc-700 hover:bg-zinc-600 disabled:opacity-50 rounded text-sm transition-colors"
                            >
                                {receiving ? 'Polling...' : 'Poll for Messages'}
                            </button>
                        </div>
                        {#if receiveError && receivedMessages.length === 0}
                            <div class="text-zinc-500 text-sm">{receiveError}</div>
                        {:else if receivedMessages.length > 0}
                            <div class="flex flex-col gap-3">
                                {#each receivedMessages as msg}
                                    <div class="bg-zinc-800 rounded border border-zinc-700 p-3">
                                        <div class="flex items-start justify-between gap-2 mb-2">
                                            <span class="text-xs text-zinc-500 font-mono">{msg.messageId}</span>
                                            <button
                                                onclick={() => handleDeleteMessage(msg.receiptHandle)}
                                                class="px-2 py-1 text-red-400 hover:text-red-300 hover:bg-red-900/30 rounded text-xs transition-colors shrink-0"
                                            >
                                                Delete
                                            </button>
                                        </div>
                                        <pre class="text-sm text-zinc-200 whitespace-pre-wrap break-all font-mono bg-zinc-900 rounded p-2 text-xs">{msg.body}</pre>
                                    </div>
                                {/each}
                            </div>
                        {:else}
                            <div class="text-zinc-600 text-sm">Click "Poll for Messages" to receive up to 10 messages.</div>
                        {/if}
                    </div>
                {:else}
                    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-500 text-sm">
                        Select a queue to view details and send/receive messages.
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
