<div class="p-6">
	<h1 class="text-2xl font-bold mb-2">ECR — Elastic Container Registry</h1>
	<p class="text-zinc-500 mb-6">Managed Docker container registry. Store, manage, and deploy container images.</p>
	<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600">
		<p>Create a repository using the AWS CLI:</p>
		<code class="block mt-2 text-sm text-orange-400 font-mono">
			aws --endpoint-url http://localhost:4566 ecr create-repository --repository-name my-repo
		</code>
	</div>
</div>
