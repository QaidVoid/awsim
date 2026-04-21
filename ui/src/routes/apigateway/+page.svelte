<div class="p-6">
    <h1 class="text-2xl font-bold mb-2">API Gateway</h1>
    <p class="text-zinc-500 mb-6">Manage REST APIs, HTTP APIs, and WebSocket APIs.</p>

    <div class="bg-zinc-900 rounded-lg border border-zinc-800 p-8 text-center text-zinc-600">
        <p class="text-zinc-400 mb-4">API Gateway management UI coming soon.</p>
        <p class="text-sm mb-2">List REST APIs via the AWS CLI:</p>
        <code class="block text-sm text-orange-400 font-mono mb-4">
            aws --endpoint-url http://localhost:4566 apigateway get-rest-apis
        </code>
        <p class="text-sm mb-2">List HTTP APIs:</p>
        <code class="block text-sm text-orange-400 font-mono">
            aws --endpoint-url http://localhost:4566 apigatewayv2 get-apis
        </code>
    </div>
</div>
