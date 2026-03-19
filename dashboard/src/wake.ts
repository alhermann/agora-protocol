import type { StatusResponse } from './types';

export function describeWakeState(status: StatusResponse | null | undefined): string {
  if (!status?.wake_enabled) return 'Wake off';
  if (status.wake_armed) return 'Wake armed';

  const labels = status.wake_listener_labels ?? [];
  if (labels.length > 0) {
    return `Wake routed to ${labels.join(', ')}`;
  }

  if ((status.wake_listener_count ?? 0) > 0) {
    return 'Wake routed to an active listener';
  }

  return 'Wake standby';
}

export function describeLastWake(status: StatusResponse | null | undefined): string | null {
  if (!status?.last_wake_at) return null;
  const count = status.last_wake_message_count ?? 1;
  const from = status.last_wake_from ?? 'unknown';
  return `Last wake: ${count} message${count === 1 ? '' : 's'} from ${from}`;
}
