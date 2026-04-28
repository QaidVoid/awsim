/**
 * Module-scope state for the global "Inspect" drawer. Lets any component
 * — request stream, request log, hotkey handler, palette — open the
 * drawer for a specific request id without prop drilling.
 */

import type { RequestEvent } from "./events";

class InspectState {
  open = $state(false);
  /** Currently inspected request id, or null when no target picked yet. */
  eventId = $state<string | null>(null);
  /** Optional SSE event for instant metadata while detail is fetched. */
  event = $state<RequestEvent | null>(null);

  show(eventId: string, event: RequestEvent | null = null) {
    this.eventId = eventId;
    this.event = event;
    this.open = true;
  }

  close() {
    this.open = false;
  }
}

export const inspectState = new InspectState();
