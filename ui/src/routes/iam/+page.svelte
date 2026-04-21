<div class="p-6">
	<h1 class="text-2xl font-bold mb-2">IAM — Identity & Access Management</h1>
	<p class="text-zinc-500 mb-6">Manage users, roles, and policies for access control.</p>
	<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600">
		<p>Create a role using the AWS CLI:</p>
		<code class="block mt-2 text-sm text-orange-400 font-mono">
			aws --endpoint-url http://localhost:4566 iam create-role --role-name my-role --assume-role-policy-document file://trust-policy.json
		</code>
	</div>
</div>
