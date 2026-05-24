<script lang="ts">
	import { goto } from "$app/navigation";
	import { setup, type AuthError, type SetupResponse } from "$lib/api/auth";
	import { route } from "$lib/url";

	let bootstrapToken = $state("");
	let password = $state("");
	let confirmPassword = $state("");
	let submitting = $state(false);
	let errorMessage = $state<string | null>(null);
	let result = $state<SetupResponse | null>(null);

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		errorMessage = null;

		if (password !== confirmPassword) {
			errorMessage = "Passwords do not match.";
			return;
		}

		submitting = true;
		try {
			result = await setup({
				bootstrap_token: bootstrapToken.trim(),
				password,
			});
		} catch (err) {
			const e = err as AuthError;
			errorMessage = e.message || "Setup failed";
		} finally {
			submitting = false;
		}
	}

	async function continueToLogin() {
		await goto(route("/login"));
	}
</script>

<svelte:head>
	<title>Set up AWSim operator</title>
</svelte:head>

<main class="setup-shell">
	{#if result}
		<section class="setup-card success">
			<h1>Setup complete</h1>
			<p class="subtitle">
				Save these access keys now. The secret access key cannot be
				retrieved again. Sign in with the password you chose to access the
				admin UI.
			</p>

			<dl class="keys">
				<dt>Principal</dt>
				<dd><code>{result.principal}</code></dd>
				<dt>Access key ID</dt>
				<dd><code>{result.access_key_id}</code></dd>
				<dt>Secret access key</dt>
				<dd><code>{result.secret_access_key}</code></dd>
			</dl>

			<button type="button" onclick={continueToLogin}>Continue to sign in</button>
		</section>
	{:else}
		<form onsubmit={handleSubmit} class="setup-card">
			<h1>First-run setup</h1>
			<p class="subtitle">
				Paste the bootstrap token printed to AWSim&apos;s stdout and pick a
				root operator password. This page runs once per data directory.
			</p>

			<label>
				<span>Bootstrap token</span>
				<input
					type="text"
					bind:value={bootstrapToken}
					required
					autocomplete="off"
					autocapitalize="off"
					autocorrect="off"
					spellcheck="false"
				/>
			</label>

			<label>
				<span>Root password</span>
				<input
					type="password"
					bind:value={password}
					required
					autocomplete="new-password"
					minlength={8}
				/>
			</label>

			<label>
				<span>Confirm password</span>
				<input
					type="password"
					bind:value={confirmPassword}
					required
					autocomplete="new-password"
					minlength={8}
				/>
			</label>

			{#if errorMessage}
				<div class="error">{errorMessage}</div>
			{/if}

			<button type="submit" disabled={submitting}>
				{submitting ? "Setting up..." : "Create root operator"}
			</button>
		</form>
	{/if}
</main>

<style>
	.setup-shell {
		min-height: 100vh;
		display: flex;
		align-items: center;
		justify-content: center;
		background: hsl(var(--background, 0 0% 100%));
		padding: 1.5rem;
	}

	.setup-card {
		width: 100%;
		max-width: 460px;
		padding: 2rem;
		border: 1px solid hsl(var(--border, 220 13% 91%));
		border-radius: 8px;
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		background: hsl(var(--card, 0 0% 100%));
	}

	.setup-card h1 {
		margin: 0;
		font-size: 1.5rem;
	}

	.subtitle {
		margin: 0 0 0.5rem 0;
		color: hsl(var(--muted-foreground, 215 16% 47%));
		font-size: 0.875rem;
		line-height: 1.45;
	}

	label {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		font-size: 0.875rem;
	}

	label span {
		color: hsl(var(--muted-foreground, 215 16% 47%));
	}

	input {
		padding: 0.5rem 0.625rem;
		border: 1px solid hsl(var(--input, 220 13% 91%));
		border-radius: 6px;
		font-size: 0.875rem;
		background: hsl(var(--background, 0 0% 100%));
		color: hsl(var(--foreground, 222.2 84% 4.9%));
		font-family: ui-monospace, SFMono-Regular, monospace;
	}

	input:focus {
		outline: 2px solid hsl(var(--ring, 222.2 84% 4.9%));
		outline-offset: -1px;
	}

	button {
		margin-top: 0.5rem;
		padding: 0.625rem;
		background: hsl(var(--primary, 222.2 47.4% 11.2%));
		color: hsl(var(--primary-foreground, 210 40% 98%));
		border: none;
		border-radius: 6px;
		font-weight: 500;
		cursor: pointer;
	}

	button:disabled {
		opacity: 0.5;
		cursor: wait;
	}

	.error {
		padding: 0.5rem 0.625rem;
		background: hsl(var(--destructive, 0 84.2% 60.2%) / 0.1);
		color: hsl(var(--destructive, 0 84.2% 60.2%));
		border-radius: 6px;
		font-size: 0.875rem;
	}

	.keys {
		display: grid;
		grid-template-columns: max-content 1fr;
		gap: 0.5rem 1rem;
		margin: 0.5rem 0 1rem 0;
		font-size: 0.875rem;
	}

	.keys dt {
		color: hsl(var(--muted-foreground, 215 16% 47%));
		font-weight: 500;
	}

	.keys dd {
		margin: 0;
		word-break: break-all;
	}

	.keys code {
		font-family: ui-monospace, SFMono-Regular, monospace;
		font-size: 0.8125rem;
		background: hsl(var(--muted, 220 13% 91%) / 0.5);
		padding: 0.125rem 0.375rem;
		border-radius: 4px;
	}
</style>
