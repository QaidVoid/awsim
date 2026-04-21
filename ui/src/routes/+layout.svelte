<script lang="ts">
	import '../app.css';

	const services = [
		{ name: 'S3', href: '/s3', icon: '📦' },
		{ name: 'DynamoDB', href: '/dynamodb', icon: '🗄️' },
		{ name: 'SQS', href: '/sqs', icon: '📬' },
		{ name: 'SNS', href: '/sns', icon: '📢' },
		{ name: 'Lambda', href: '/lambda', icon: 'λ' },
		{ name: 'IAM', href: '/iam', icon: '🔑' },
		{ name: 'CloudWatch', href: '/cloudwatch', icon: '📊' },
		{ name: 'EventBridge', href: '/eventbridge', icon: '⚡' },
		{ name: 'KMS', href: '/kms', icon: '🔐' },
		{ name: 'Secrets', href: '/secrets', icon: '🤫' },
		{ name: 'SSM', href: '/ssm', icon: '⚙️' },
		{ name: 'STS', href: '/sts', icon: '🎫' },
		{ name: 'Kinesis', href: '/kinesis', icon: '🌊' },
		{ name: 'Step Functions', href: '/stepfunctions', icon: '🔄' },
		{ name: 'SES', href: '/ses', icon: '✉️' },
		{ name: 'Cognito', href: '/cognito', icon: '👤' },
		{ name: 'EC2', href: '/ec2', icon: '🖥️' },
		{ name: 'RDS', href: '/rds', icon: '🗃️' },
		{ name: 'ECS', href: '/ecs', icon: '🐳' },
		{ name: 'ECR', href: '/ecr', icon: '📋' },
		{ name: 'CloudFormation', href: '/cloudformation', icon: '🏗️' },
		{ name: 'ELB', href: '/elb', icon: '⚖️' },
		{ name: 'CloudFront', href: '/cloudfront', icon: '🌍' },
		{ name: 'API Gateway', href: '/apigateway', icon: '🌐' },
		{ name: 'AppSync', href: '/appsync', icon: '🔗' },
		{ name: 'Bedrock', href: '/bedrock', icon: '🪨' },
		{ name: 'Route53', href: '/route53', icon: '🌐' },
		{ name: 'CloudWatch Metrics', href: '/monitoring', icon: '📈' },
		{ name: 'Athena', href: '/athena', icon: '🔍' },
		{ name: 'Glue', href: '/glue', icon: '🗂️' },
		{ name: 'Request Log', href: '/logs', icon: '📋' },
	];

	let { children } = $props();
</script>

<!-- Dark theme with zinc colors -->
<div class="flex h-screen bg-zinc-950 text-zinc-100">
	<!-- Sidebar -->
	<aside class="w-56 bg-zinc-900 border-r border-zinc-800 flex flex-col shrink-0 overflow-y-auto">
		<div class="p-4 border-b border-zinc-800">
			<a href="/" class="text-xl font-bold text-orange-400 hover:text-orange-300">AWSim</a>
			<p class="text-xs text-zinc-500 mt-1">Local AWS Emulator</p>
		</div>
		<nav class="flex-1 py-2">
			{#each services as svc}
				<a href={svc.href}
				   class="flex items-center gap-2 px-4 py-1.5 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 transition-colors">
					<span class="w-5 text-center text-xs">{svc.icon}</span>
					{svc.name}
				</a>
			{/each}
		</nav>
		<div class="p-3 border-t border-zinc-800 text-xs text-zinc-600">
			v0.1.0
		</div>
	</aside>

	<!-- Main content -->
	<main class="flex-1 overflow-y-auto">
		{@render children()}
	</main>
</div>
