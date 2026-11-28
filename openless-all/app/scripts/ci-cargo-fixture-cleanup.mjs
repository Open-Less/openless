import { rmSync } from 'node:fs';

export function withCargoFixtureCleanup(directory, run, remove = rmSync) {
  let failed = false;
  let failure;
  try {
    return run();
  } catch (error) {
    failed = true;
    failure = error;
    throw error;
  } finally {
    try {
      // Retry the observed Windows busy removals only in this Cargo fixture.
      // Use Node's bounded retry budget; exhaustion still throws.
      remove(directory, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
    } catch (cleanupError) {
      if (failed) {
        throw new AggregateError(
          [failure, cleanupError],
          'Cargo fixture failed and its temporary directory could not be removed',
          { cause: failure },
        );
      }
      throw cleanupError;
    }
  }
}
