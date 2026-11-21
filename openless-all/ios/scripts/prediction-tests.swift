import Foundation

@main
struct PredictionTests {
  static func main() {
    let dictionary = URL(fileURLWithPath: CommandLine.arguments[1])
    let engine = KeyboardPredictionEngine(url: dictionary)
    typealias Input = KeyboardPredictionEngine.EnglishInput
    precondition(Input.parse("I don't").prefix == "don't")
    precondition(Input.parse("I don’t").prefix == "don’t")
    precondition(Input.parse("don’t ").previousWord == "don't")
    precondition(Input.parse("at the café").prefix == "café")
    precondition(Input.parse("a×b").prefix == "b")
    precondition(Input.parse("Thank ").previousWord == "thank")
    precondition(Input.parse("hello.\n").previousWord.isEmpty)
    precondition(Input.parse("Hello ").prefix.isEmpty)
    precondition(engine.englishCandidates(for: Input.parse("Thank ")).first == "you")
    precondition(engine.englishCandidates(for: Input.parse("Thank y")).contains("you"))
    precondition(engine.englishCandidates(for: Input.parse("helo"), corrections: ["hello"]).contains("Hello"))
    precondition(engine.englishCandidates(for: Input.parse("HEL")).contains("HELLO"))
    precondition(engine.englishCandidates(for: Input.parse("")).isEmpty)
    precondition(engine.englishCandidates(for: Input.parse("Hello. ")).isEmpty)
    precondition(engine.englishCandidates(for: Input.parse("Thanks ")).count <= 8)
    precondition(engine.chineseCandidates(before: "你好").prefix(2) == ["吗", "啊"])
    precondition(engine.chineseCandidates(before: "中国").first == "人")
    precondition(engine.chineseCandidates(before: "我说你好").first == "吗")
    precondition(engine.chineseCandidates(before: "你好 ").isEmpty)
    precondition(engine.chineseCandidates(before: "你好。").isEmpty)
    // A candidate must append just the remaining suffix, never duplicate input.
    let phrase = "你好" + engine.chineseCandidates(before: "你好")[0]
    precondition(phrase == "你好吗")
    let fallback = KeyboardPredictionEngine(url: nil)
    precondition(fallback.englishCandidates(for: Input.parse("hel"), completions: ["hello"]) == ["hel", "Hello"])
    precondition(fallback.chineseCandidates(before: "中国").isEmpty)
    precondition(KeyboardPredictionEngine.reading(for: "张三") == "zhangsan")
    precondition(KeyboardPredictionEngine.reading(for: "绿茶") == "lvcha")
    precondition(KeyboardPredictionEngine.reading(for: "John") == nil)
    engine.setSupplementaryEntries([
      .init(input: "omw", text: "On my way!"),
      .init(input: "Alexandra", text: "Alexandra"),
      .init(input: "iphone", text: "iPhone"),
      .init(input: "张三", text: "张三", reading: "zhangsan"),
      .init(input: "bad", text: "line\nbreak"),
    ])
    precondition(engine.englishCandidates(for: Input.parse("omw")).prefix(2) == ["omw", "On my way!"])
    precondition(engine.supplementaryCandidates(for: "al").first == "Alexandra")
    precondition(engine.englishCandidates(for: Input.parse("iphone")).prefix(2) == ["iphone", "iPhone"])
    precondition(engine.supplementaryCandidates(for: "zhang's", chinese: true).first == "张三")
    precondition(engine.supplementaryCandidates(for: "bad").isEmpty)
    precondition(engine.englishCandidates(for: Input.parse("teh"), corrections: ["the"]).prefix(2) == ["teh", "The"])
    engine.setSupplementaryEntries([])
    precondition(engine.supplementaryCandidates(for: "omw").isEmpty)

    engine.setSupplementaryEntries((0..<2048).map {
      .init(input: "entry\($0)", text: "Name \($0)")
    })
    var latencies: [Double] = []
    let samples = ["a", "I ", "Thank ", "th", "hello", "Where ", "com", "I don't", "w"]
    for index in 0..<450 {
      let start = Date()
      _ = engine.englishCandidates(for: Input.parse(samples[index % samples.count]))
      _ = engine.chineseCandidates(before: index.isMultiple(of: 2) ? "中国" : "我说你好")
      latencies.append(Date().timeIntervalSince(start) * 1_000)
    }
    latencies.sort()
    print(String(format: "Prediction checks passed; combined English + Chinese query p50 %.2f ms, p95 %.2f ms (host, 450 iterations).",
      latencies[latencies.count / 2], latencies[Int(Double(latencies.count) * 0.95)]))
  }
}
