import Combine
import Foundation

/// Main-app product data. Core remains the only writer of history, activity and
/// vocabulary. No preview or pending result silently replaces a stored original.
final class ProductLibrary: ObservableObject {
  @Published private(set) var history: [CoreHistoryEntry] = []
  @Published private(set) var activity: [CoreActivityDay] = []
  @Published private(set) var styles: [LibraryStyle] = []
  @Published private(set) var canPolish = false
  @Published private(set) var hasLoaded = false
  @Published private(set) var loading = false
  @Published private(set) var busy = false
  @Published var error: String?
  @Published private(set) var vocabulary: [CoreVocabularyEntry] = []
  @Published private(set) var vocabularyTotal = 0
  @Published private(set) var vocabularyLoading = false
  @Published private(set) var vocabularyLoaded = false
  @Published private(set) var vocabularyError: String?
  @Published private(set) var historyResults: [CoreHistoryEntry] = []
  @Published private(set) var historySearching = false
  @Published private(set) var historySearchError: String?
  @Published private(set) var historyQuery = ""
  private var historySearchID = UUID()
  private weak var core: CoreBridge?
  private var vocabularySelection = VocabularySelection()
  private var vocabularyRequestID = UUID()
  private var refreshRequestID = UUID()

  func connect(_ core: CoreBridge?) {
    self.core = core; history = []; activity = []; styles = []; vocabulary = []
    vocabularyTotal = 0; canPolish = false; hasLoaded = false; error = nil
    vocabularyLoading = false; vocabularyLoaded = false; vocabularyError = nil
    vocabularySelection = VocabularySelection(); vocabularyRequestID = UUID()
    loading = false; refreshRequestID = UUID()
    historyResults = []; historyQuery = ""; historySearching = false; historySearchError = nil; historySearchID = UUID()
  }
  private func request<T: Decodable>(_ action: [String: Any]) async throws -> T {
    guard let core else { throw DeveloperError.unavailable }
    let data = try await core.control(action)
    try Task.checkCancellation()
    guard self.core === core else { throw CancellationError() }
    return try JSONDecoder().decode(T.self, from: data)
  }
  func refresh() async {
    guard !loading else { return }
    loading = true
    let requestID = UUID(); refreshRequestID = requestID
    defer { if refreshRequestID == requestID { loading = false } }
    struct Snapshot: Decodable {
      let history: [CoreHistoryEntry]; let activity: [CoreActivityDay]
      let styles: [LibraryStyle]; let canPolish: Bool
    }
    do {
      let snapshot: Snapshot = try await request(["action": "library"])
      history = HistoryFormatting.newestFirst(snapshot.history); activity = snapshot.activity; styles = snapshot.styles
      canPolish = snapshot.canPolish; hasLoaded = true; error = nil
    } catch is CancellationError {} catch {
      if refreshRequestID == requestID { self.error = error.localizedDescription }
    }
  }
  func searchHistory(_ query: String) async {
    struct Result: Decodable { let entries: [CoreHistoryEntry] }
    let id = UUID(); historySearchID = id
    historySearching = true; historySearchError = nil
    defer { if historySearchID == id { historySearching = false } }
    do {
      let result: Result = try await request(["action": "searchHistory", "query": query])
      guard historySearchID == id else { return }
      historyResults = HistoryFormatting.newestFirst(result.entries); historyQuery = query
    } catch is CancellationError {} catch {
      if historySearchID == id { historySearchError = error.localizedDescription }
    }
  }
  func detail(_ id: String) async throws -> CoreHistoryEntry {
    try await request(["action": "historyDetail", "id": id])
  }
  func repolish(_ id: String, style: String) async throws -> String {
    guard !busy else { throw DeveloperError.rejected("请等待当前操作完成。") }
    busy = true; defer { busy = false }
    struct Result: Decodable { let text: String }
    let result: Result = try await request(["action": "repolishHistory", "id": id, "stylePackId": style])
    return result.text
  }
  func audioURL(_ id: String) async throws -> URL {
    struct Asset: Decodable { let path: String }
    let asset: Asset = try await request(["action": "audioAsset", "id": id])
    return URL(fileURLWithPath: asset.path)
  }
  @discardableResult func mutate(_ action: [String: Any]) async -> Bool {
    guard !busy else { return false }
    busy = true; error = nil; defer { busy = false }
    do {
      guard let core else { throw DeveloperError.unavailable }
      _ = try await core.control(action)
      guard self.core === core else { throw CancellationError() }
      await refresh()
      return true
    } catch is CancellationError { return false }
      catch { self.error = error.localizedDescription; return false }
  }
  func vocabularyMatches(query: String, source: VocabularySource) -> Bool {
    vocabularySelection == VocabularySelection(query: query, source: source)
  }
  func saveVocabulary(phrase: String, entryID: String?) async -> Bool {
    guard VocabularyInput.validationMessage(phrase) == nil else { return false }
    return await mutate(VocabularyInput.saveRequest(phrase, entryID: entryID))
  }
  func loadVocabulary(query: String = "", source: VocabularySource = .all, more: Bool = false) async {
    struct Page: Decodable { let entries: [CoreVocabularyEntry]; let total: Int }
    let selection = VocabularySelection(query: query, source: source)
    if more && (vocabularySelection != selection || vocabularyLoading || !vocabularyLoaded) { return }
    if vocabularySelection != selection {
      vocabulary = []; vocabularyTotal = 0; vocabularyLoaded = false
    }
    vocabularySelection = selection; vocabularyError = nil
    let requestID = UUID(); vocabularyRequestID = requestID; vocabularyLoading = true
    defer { if vocabularyRequestID == requestID { vocabularyLoading = false } }
    do {
      let page: Page = try await request(selection.request(offset: more ? vocabulary.count : 0))
      guard vocabularyRequestID == requestID else { return }
      if more { vocabulary += page.entries } else { vocabulary = page.entries }
      vocabularyTotal = page.total; vocabularyLoaded = true
    } catch is CancellationError {} catch {
      if vocabularyRequestID == requestID { vocabularyError = error.localizedDescription }
    }
  }
}
