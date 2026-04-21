<div class="p-6">
	<h1 class="text-2xl font-bold mb-2">SSM — Parameter Store</h1>
	<p class="text-zinc-500 mb-6">Systems Manager Parameter Store. Store configuration and secrets hierarchically.</p>
	<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600">
		<p>Put a parameter using the AWS CLI:</p>
		<code class="block mt-2 text-sm text-orange-400 font-mono">
			aws --endpoint-url http://localhost:4566 ssm put-parameter --name /my/app/config --value "hello" --type String
		</code>
	</div>
</div>
