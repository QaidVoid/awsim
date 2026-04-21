<div class="p-6">
	<h1 class="text-2xl font-bold mb-2">SES — Simple Email Service</h1>
	<p class="text-zinc-500 mb-6">Send transactional and marketing emails at scale.</p>
	<div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600">
		<p>Send an email using the AWS CLI:</p>
		<code class="block mt-2 text-sm text-orange-400 font-mono">
			aws --endpoint-url http://localhost:4566 ses send-email --from sender@example.com --destination ToAddresses=recipient@example.com --message Subject=&#123;Data=Hello&#125;,Body=&#123;Text=&#123;Data="World"&#125;&#125;
		</code>
	</div>
</div>
