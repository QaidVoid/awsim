<div class="p-6">
	<h1 class="text-2xl font-bold mb-2">Kinesis — Data Streams</h1>
	<p class="text-zinc-500 mb-6">Real-time data streaming. Collect, process, and analyze data streams.</p>
	<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600">
		<p>Create a stream using the AWS CLI:</p>
		<code class="block mt-2 text-sm text-orange-400 font-mono">
			aws --endpoint-url http://localhost:4566 kinesis create-stream --stream-name my-stream --shard-count 1
		</code>
	</div>
</div>
