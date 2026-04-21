<div class="p-6">
	<h1 class="text-2xl font-bold mb-2">DynamoDB — Tables</h1>
	<p class="text-zinc-500 mb-6">Managed NoSQL database. Browse tables, items, and indexes.</p>
	<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600">
		<p>No tables yet. Create one using the AWS CLI:</p>
		<code class="block mt-2 text-sm text-orange-400 font-mono">
			aws --endpoint-url http://localhost:4566 dynamodb create-table --table-name my-table --attribute-definitions AttributeName=id,AttributeType=S --key-schema AttributeName=id,KeyType=HASH --billing-mode PAY_PER_REQUEST
		</code>
	</div>
</div>
