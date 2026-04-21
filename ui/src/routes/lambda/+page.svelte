<div class="p-6">
	<h1 class="text-2xl font-bold mb-2">Lambda — Functions</h1>
	<p class="text-zinc-500 mb-6">Serverless compute. Run code without provisioning servers.</p>
	<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600">
		<p>No functions yet. Create one using the AWS CLI:</p>
		<code class="block mt-2 text-sm text-orange-400 font-mono">
			aws --endpoint-url http://localhost:4566 lambda create-function --function-name my-fn --runtime nodejs20.x --role arn:aws:iam::000000000000:role/exec --handler index.handler --zip-file fileb://function.zip
		</code>
	</div>
</div>
