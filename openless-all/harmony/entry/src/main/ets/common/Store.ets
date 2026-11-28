/** Centralized application-state assignment.
 * AppStorage.setOrCreate updates existing properties and creates missing ones.
 * https://developer.huawei.com/consumer/cn/doc/harmonyos-references/ts-state-management
 */
export class Store {
  static put<T>(key: string, value: T): void {
    AppStorage.setOrCreate<T>(key, value);
  }
}
