/**
 * One-shot navigation intent.
 *
 * The command palette's "Create X" quick actions record a pending
 * action id and then navigate; the destination page consumes it on
 * mount to auto-open its create dialog. Kept off the URL on purpose so
 * it never collides with pages (S3, DynamoDB, ...) that sync their own
 * query params via $effect.
 */

let pending = $state<string | null>(null);

export const pendingAction = {
  /** The currently queued action id, if any. */
  get current(): string | null {
    return pending;
  },
  /** Queue `id` to be consumed by the next page that looks for it. */
  request(id: string): void {
    pending = id;
  },
  /** If `id` is queued, clear it and return true; otherwise false. */
  consume(id: string): boolean {
    if (pending !== id) return false;
    pending = null;
    return true;
  },
};
