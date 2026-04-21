<div class="p-6">
	<h1 class="text-2xl font-bold mb-2">Step Functions — State Machines</h1>
	<p class="text-zinc-500 mb-6">Coordinate distributed applications using visual workflows.</p>
	<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600">
		<p>Create a state machine using the AWS CLI:</p>
		<code class="block mt-2 text-sm text-orange-400 font-mono">
			aws --endpoint-url http://localhost:4566 stepfunctions create-state-machine --name my-machine --definition file://definition.json --role-arn arn:aws:iam::000000000000:role/exec
		</code>
	</div>
</div>
