// Openless Core ML adapter for the pinned English EG2 STT artifacts.
// No dependency on the third-party application or its private Swift ABI.
// Frontend: Gemma4AudioFeatureExtractor (16 kHz, semicausal Hann STFT,
// magnitude mel, natural log + .001), with the artifact's supplied filters.
import Foundation
import CoreML
import Accelerate

private enum GemmaError: Error { case invalid(String), cancelled }

@available(macOS 15.0, *)
private final class GemmaRecognizer {
    let encoder: MLModel
    let reader: MLModel
    let scorer: MLModel
    let punctuation: MLModel
    let words: [String]
    let table: Data
    let hann: [Float]
    let filters: [Float]
    let scale: Float
    let bias: Float
    let fft: vDSP_DFT_Setup

    init(folder: URL, checkpoint: () throws -> Void) throws {
        let config = MLModelConfiguration()
        // Core ML chooses supported ANE/GPU/CPU partitions. Never claim ANE-only.
        config.computeUnits = .all
        func load(_ name: String) throws -> MLModel {
            try checkpoint()
            let model = try MLModel(contentsOf: folder.appendingPathComponent(name), configuration: config)
            try checkpoint()
            return model
        }
        encoder = try load("eg2_audio_l11.mlmodelc")
        reader = try load("reader_P6.mlmodelc")
        scorer = try load("scorer_blk_d128_k16_b4_t500.mlmodelc")
        punctuation = try load("punct_headonly.mlmodelc")
        words = try JSONDecoder().decode([String].self, from: Data(contentsOf: folder.appendingPathComponent("reader_words.json")))
        table = try Data(contentsOf: folder.appendingPathComponent("word_unit_f16.bin"), options: .mappedIfSafe)
        func floats(_ name: String) throws -> [Float] {
            let data = try Data(contentsOf: folder.appendingPathComponent(name))
            guard data.count % 4 == 0 else { throw GemmaError.invalid(name) }
            return data.withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
        }
        hann = try floats("hann_320.f32")
        filters = try floats("mel_filters_257x128.f32")
        struct ReaderConfig: Decodable { let scale: Float; let bias: Float }
        let rc = try JSONDecoder().decode(ReaderConfig.self, from: Data(contentsOf: folder.appendingPathComponent("reader_P6.json")))
        scale = rc.scale; bias = rc.bias
        guard words.count == 229457, table.count == words.count * 768 * 2,
              hann.count == 320, filters.count == 257 * 128,
              let plan = vDSP_DFT_zop_CreateSetup(nil, 512, .FORWARD) else {
            throw GemmaError.invalid("Invalid English model package")
        }
        fft = plan
    }
    deinit { vDSP_DFT_DestroySetup(fft) }
    func tensor(_ shape: [Int], _ type: MLMultiArrayDataType = .float32) throws -> MLMultiArray {
        let a = try MLMultiArray(shape: shape.map(NSNumber.init), dataType: type)
        memset(a.dataPointer, 0, a.count * (type == .float16 ? 2 : 4))
        return a
    }
    func output(_ model: MLModel, _ inputs: [String: MLMultiArray], _ key: String,
                checkpoint: () throws -> Void) throws -> MLMultiArray {
        try checkpoint()
        let values = inputs.mapValues { MLFeatureValue(multiArray: $0) }
        let prediction = try model.prediction(from: MLDictionaryFeatureProvider(dictionary: values))
        try checkpoint()
        guard let result = prediction.featureValue(for: key)?.multiArrayValue else {
            throw GemmaError.invalid("Missing model output: \(key)")
        }
        return result
    }
    func mel(_ audio: ArraySlice<Float>) throws -> (MLMultiArray, MLMultiArray, Int) {
        let features = try tensor([1, 2000, 128])
        let mask = try tensor([1, 2000])
        let input = Array(audio)
        // A feature is valid only when its last real sample is available.
        let count = max(0, min(2000, (input.count - 1) / 160))
        var real = [Float](repeating: 0, count: 512)
        let imag = [Float](repeating: 0, count: 512)
        var r = real, i = real
        var magnitude = [Float](repeating: 0, count: count * 257)
        for frame in 0..<count {
            for n in 0..<320 {
                let index = frame * 160 + n - 160
                real[n] = (index >= 0 && index < input.count ? input[index] : 0) * hann[n]
            }
            vDSP_DFT_Execute(fft, real, imag, &r, &i)
            for bin in 0..<257 { magnitude[frame * 257 + bin] = hypotf(r[bin], i[bin]) }
        }
        let data = features.dataPointer.assumingMemoryBound(to: Float.self)
        if count > 0 {
            cblas_sgemm(CblasRowMajor, CblasNoTrans, CblasNoTrans, Int32(count), 128, 257,
                        1, magnitude, 257, filters, 128, 0, data, 128)
            for n in 0..<(count * 128) { data[n] = logf(data[n] + 0.001) }
            let mp = mask.dataPointer.assumingMemoryBound(to: Float.self)
            for n in 0..<count { mp[n] = 1 }
        }
        return (features, mask, min(500, (count + 3) / 4))
    }
    func window(_ audio: ArraySlice<Float>, checkpoint: () throws -> Void) throws -> [Int] {
        try checkpoint()
        let (features, mask, frames) = try mel(audio)
        try checkpoint()
        if frames == 0 { return [] }
        let encoded = try output(encoder, ["input_features": features, "feature_mask": mask], "l11", checkpoint: checkpoint)
        let l11 = try tensor([1, 500, 1024])
        for n in 0..<l11.count { l11[n] = encoded[n] }
        let readerMask = try tensor([1, 500])
        for n in 0..<frames { readerMask[n] = 1 }
        let values = try reader.prediction(from: MLDictionaryFeatureProvider(dictionary: [
            "l11": MLFeatureValue(multiArray: l11), "mask": MLFeatureValue(multiArray: readerMask),
        ]))
        try checkpoint()
        guard let vectors = values.featureValue(for: "vec")?.multiArrayValue,
              let blanks = values.featureValue(for: "blank")?.multiArrayValue,
              vectors.dataType == .float16 else { throw GemmaError.invalid("Invalid reader outputs") }
        let blocks = try output(scorer, ["vec": vectors], "bidx", checkpoint: checkpoint)
        guard blocks.dataType == .int32 else { throw GemmaError.invalid("Invalid scorer output") }
        let vp = vectors.dataPointer.assumingMemoryBound(to: Float16.self)
        let bp = blocks.dataPointer.assumingMemoryBound(to: Int32.self)
        let vs = vectors.strides.map(\.intValue), bs = blocks.strides.map(\.intValue)
        return try table.withUnsafeBytes { storage in
            let wp = storage.bindMemory(to: Float16.self)
            var result = [Int](repeating: -1, count: frames)
            for frame in 0..<frames {
                if frame.isMultiple(of: 16) { try checkpoint() }
                var best = -Float.infinity, bestID = -1
                // The Core ML scorer returns the top 16 blocks of 4 words.
                // Rerank their full 768-dimensional FP16 vectors, not a 229k logits tensor.
                for candidate in 0..<16 {
                    let block = Int(bp[frame * bs[1] + candidate * bs[2]])
                    for offset in 0..<4 {
                        let id = block * 4 + offset
                        if id < 0 || id >= words.count { continue }
                        var score: Float = 0
                        for dim in 0..<768 { score += Float(vp[frame * vs[1] + dim * vs[2]]) * Float(wp[id * 768 + dim]) }
                        if score > best { best = score; bestID = id }
                    }
                }
                if best * scale + bias > blanks[frame].floatValue { result[frame] = bestID }
            }
            return result
        }
    }
    func punctuate(_ ids: [Int], checkpoint: () throws -> Void) throws -> String {
        var text = ""
        for start in stride(from: 0, to: ids.count, by: 96) {
            try checkpoint()
            let part = Array(ids[start..<min(start + 96, ids.count)])
            let vectors = try tensor([1, 96, 768], .float16), mask = try tensor([1, 96], .float16)
            table.withUnsafeBytes { raw in
                let wp = raw.bindMemory(to: Float16.self)
                let target = vectors.dataPointer.assumingMemoryBound(to: Float16.self)
                for (n, id) in part.enumerated() {
                    target.advanced(by: n * 768).update(from: wp.baseAddress!.advanced(by: id * 768), count: 768)
                    mask[n] = 1
                }
            }
            let output = try punctuation.prediction(from: MLDictionaryFeatureProvider(dictionary: [
                "vec": MLFeatureValue(multiArray: vectors), "mask": MLFeatureValue(multiArray: mask),
            ]))
            try checkpoint()
            guard let punct = output.featureValue(for: "punct")?.multiArrayValue,
                  let casing = output.featureValue(for: "case")?.multiArrayValue else { throw GemmaError.invalid("Invalid punctuation output") }
            func argmax(_ a: MLMultiArray, _ n: Int, _ width: Int) -> Int {
                (0..<width).max { a[n * width + $0].floatValue < a[n * width + $1].floatValue } ?? 0
            }
            for (n, id) in part.enumerated() {
                var word = words[id]
                let style = argmax(casing, n, 3)
                if style == 2 { word = word.uppercased() }
                else if style == 1 || text.isEmpty { word = word.prefix(1).uppercased() + word.dropFirst() }
                if !text.isEmpty { text += " " }
                text += word + ["", ",", ".", "?"][argmax(punct, n, 4)]
            }
        }
        return text
    }
    func transcribe(_ samples: [Float], checkpoint: () throws -> Void) throws -> String {
        // Limit only this caller's CPU BLAS work, never the whole app. Core ML
        // remains .all so supported GPU/ANE partitions are still available.
        let threading = BLASGetThreading()
        BLASSetThreading(BLAS_THREADING_SINGLE_THREADED)
        defer { BLASSetThreading(threading) }
        try checkpoint()
        guard !samples.isEmpty else { return "" }
        // Short-silence guard prevents turning empty recordings into words.
        guard samples.contains(where: { abs($0) > 0.0001 }) else { return "" }
        var ids = [Int](), previous = -1
        var start = 0
        while start < samples.count {
            let end = min(start + 320000, samples.count)
            let decoded = try autoreleasepool { try window(samples[start..<end], checkpoint: checkpoint) }
            let lower = start == 0 ? 0 : 25
            let upper = end == samples.count ? decoded.count : min(decoded.count, 475)
            if lower < upper {
                for id in decoded[lower..<upper] {
                    if id >= 0 && id != previous { ids.append(id) }
                    previous = id
                }
            }
            if end == samples.count { break }
            start += 288000
        }
        return try punctuate(ids, checkpoint: checkpoint)
    }
}

@_cdecl("openless_gemma_load")
public func gemmaLoad(_ path: UnsafePointer<CChar>, _ checkpointContext: UnsafeMutableRawPointer,
                      _ checkpoint: @convention(c) (UnsafeMutableRawPointer) -> Bool,
                      _ error: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>) -> UnsafeMutableRawPointer? {
    guard #available(macOS 15.0, *) else { error.pointee = strdup("Gemma requires macOS 15 or later"); return nil }
    do {
        let step = {
            guard checkpoint(checkpointContext) else { throw GemmaError.cancelled }
        }
        return Unmanaged.passRetained(try GemmaRecognizer(folder: URL(fileURLWithPath: String(cString: path)), checkpoint: step)).toOpaque()
    }
    catch let failure { error.pointee = strdup(String(describing: failure)); return nil }
}
@_cdecl("openless_gemma_transcribe")
public func gemmaTranscribe(_ handle: UnsafeMutableRawPointer, _ samples: UnsafePointer<Float>, _ count: Int,
                           _ checkpointContext: UnsafeMutableRawPointer,
                           _ checkpoint: @convention(c) (UnsafeMutableRawPointer) -> Bool,
                           _ error: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>) -> UnsafeMutablePointer<CChar>? {
    guard #available(macOS 15.0, *) else { error.pointee = strdup("Gemma requires macOS 15 or later"); return nil }
    do {
        let model = Unmanaged<GemmaRecognizer>.fromOpaque(handle).takeUnretainedValue()
        // This callback is synchronous, stays on this worker, and is never
        // retained: Rust owns its context for the duration of this FFI call.
        let step = {
            guard checkpoint(checkpointContext) else { throw GemmaError.cancelled }
        }
        return try autoreleasepool { strdup(try model.transcribe(Array(UnsafeBufferPointer(start: samples, count: count)), checkpoint: step)) }
    } catch let failure { error.pointee = strdup(String(describing: failure)); return nil }
}
@_cdecl("openless_gemma_release")
public func gemmaRelease(_ handle: UnsafeMutableRawPointer) {
    if #available(macOS 15.0, *) { Unmanaged<GemmaRecognizer>.fromOpaque(handle).release() }
}
