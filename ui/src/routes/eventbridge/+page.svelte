<div class="p-6">
	<h1 class="text-2xl font-bold mb-2">EventBridge — Event Buses & Rules</h1>
	<p class="text-zinc-500 mb-6">Serverless event bus. Route events between services using rules.</p>
	<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600">
		<p>Put an event using the AWS CLI:</p>
		<code class="block mt-2 text-sm text-orange-400 font-mono">
			aws --endpoint-url http://localhost:4566 events put-events --entries Source=my.app,DetailType=MyEvent,Detail='&#123;"key":"value"&#125;'
		</code>
	</div>
</div>
