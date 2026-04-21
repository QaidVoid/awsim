<div class="p-6">
	<h1 class="text-2xl font-bold mb-2">CloudFormation — Stacks</h1>
	<p class="text-zinc-500 mb-6">Infrastructure as code. Model and provision AWS resources using templates.</p>
	<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600">
		<p>Create a stack using the AWS CLI:</p>
		<code class="block mt-2 text-sm text-orange-400 font-mono">
			aws --endpoint-url http://localhost:4566 cloudformation create-stack --stack-name my-stack --template-body file://template.yaml
		</code>
	</div>
</div>
