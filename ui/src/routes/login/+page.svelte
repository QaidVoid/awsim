<script lang="ts">
	import { goto } from "$app/navigation";
	import { login, type AuthError } from "$lib/api/auth";
	import { auth } from "$lib/auth-state.svelte";
	import { route } from "$lib/url";

	let username = $state("");
	let password = $state("");
	let mfaCode = $state("");
	let submitting = $state(false);
	let errorMessage = $state<string | null>(null);

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		submitting = true;
		errorMessage = null;
		try {
			await login({
				username,
				password,
				mfa_code: mfaCode.trim() ? mfaCode.trim() : undefined,
			});
			await auth.refresh();
			await goto(route("/"));
		} catch (err) {
			const e = err as AuthError;
			errorMessage = e.message || "Login failed";
			if (e.retry_after) {
				errorMessage = `${errorMessage} (retry in ${e.retry_after}s)`;
			}
		} finally {
			submitting = false;
		}
	}
</script>

<svelte:head>
	<title>Sign in to AWSim</title>
</svelte:head>

<main class="login-shell">
	<form onsubmit={handleSubmit} class="login-card">
		<h1>Sign in</h1>
		<p class="subtitle">Use your IAM user credentials.</p>

		<label>
			<span>Username</span>
			<input
				type="text"
				bind:value={username}
				required
				autocomplete="username"
				autocapitalize="off"
				autocorrect="off"
				spellcheck="false"
			/>
		</label>

		<label>
			<span>Password</span>
			<input
				type="password"
				bind:value={password}
				required
				autocomplete="current-password"
			/>
		</label>

		<label>
			<span>MFA code (if enabled)</span>
			<input
				type="text"
				bind:value={mfaCode}
				inputmode="numeric"
				pattern="[0-9]{'{6}'}"
				autocomplete="one-time-code"
			/>
		</label>

		{#if errorMessage}
			<div class="error">{errorMessage}</div>
		{/if}

		<button type="submit" disabled={submitting}>
			{submitting ? "Signing in..." : "Sign in"}
		</button>
	</form>
</main>

<style>
	.login-shell {
		min-height: 100vh;
		display: flex;
		align-items: center;
		justify-content: center;
		background: hsl(var(--background, 0 0% 100%));
	}

	.login-card {
		width: 100%;
		max-width: 380px;
		padding: 2rem;
		border: 1px solid hsl(var(--border, 220 13% 91%));
		border-radius: 8px;
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		background: hsl(var(--card, 0 0% 100%));
	}

	.login-card h1 {
		margin: 0;
		font-size: 1.5rem;
	}

	.subtitle {
		margin: 0 0 0.5rem 0;
		color: hsl(var(--muted-foreground, 215 16% 47%));
		font-size: 0.875rem;
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
</style>
