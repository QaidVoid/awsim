<div class="p-6">
	<h1 class="text-2xl font-bold mb-2">Secrets Manager — Secrets</h1>
	<p class="text-zinc-500 mb-6">Securely store and retrieve secrets like API keys and passwords.</p>
	<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600">
		<p>Create a secret using the AWS CLI:</p>
		<code class="block mt-2 text-sm text-orange-400 font-mono">
			aws --endpoint-url http://localhost:4566 secretsmanager create-secret --name my-secret --secret-string '&#123;"password":"hunter2"&#125;'
		</code>
	</div>
</div>
