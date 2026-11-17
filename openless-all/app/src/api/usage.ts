import { serviceUsageQuery } from '../data/usage';
import { clearServiceUsage as clear } from '../lib/ipc/service-usage';

export {
  getServiceUsage,
  supportsServiceUsage,
  type ServiceUsageRecord,
  type ServiceUsageSnapshot,
} from '../lib/ipc/service-usage';

export async function clearServiceUsage(): Promise<void> {
  await clear();
  serviceUsageQuery.invalidate();
}
