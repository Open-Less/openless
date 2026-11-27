/**
 * `AppStorage` writes.
 *
 * `setOrCreate` only creates: it silently ignores a key that already exists, so
 * using it for navigation leaves the UI on whatever the first write set. `put`
 * creates the key when needed and always assigns, which is what every writer in
 * this app means.
 */
export class Store {
  static put<T>(key: string, value: T): void {
    AppStorage.setOrCreate<T>(key, value);
    AppStorage.set<T>(key, value);
  }
}
