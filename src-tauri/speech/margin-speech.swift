// margin-speech: the Apple Speech live transcript for Margin (DECISIONS.md D56).
//
// Margin starts one process for each audio source of a recording. The process reads
// 16 kHz mono float32 little-endian samples on stdin, as the recording grows, and writes
// one JSON line on stdout for each final result:
//
//     {"start":12.3,"end":15.8,"text":"..."}
//
// Times are seconds from the first sample. At end of input it finalizes and exits.
//
//     margin-speech check <locale>    exit 0 when the locale is usable now
//     margin-speech install <locale>  download the speech assets for the locale
//     margin-speech stream <locale>   transcribe stdin as described above
//
// Errors go to stderr, with exit code 2 for "not available on this Mac".

import AVFoundation
import Foundation
import Speech

func fail(_ message: String, code: Int32 = 1) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(code)
}

@available(macOS 26.0, *)
func transcriber(for identifier: String) async -> SpeechTranscriber {
    guard SpeechTranscriber.isAvailable else {
        fail("SpeechTranscriber is not available on this Mac.", code: 2)
    }
    guard let locale = await SpeechTranscriber.supportedLocale(equivalentTo: Locale(identifier: identifier)) else {
        fail("Apple Speech does not support \(identifier).", code: 2)
    }
    return SpeechTranscriber(locale: locale, transcriptionOptions: [], reportingOptions: [], attributeOptions: [])
}

@available(macOS 26.0, *)
func check(_ identifier: String) async {
    let module = await transcriber(for: identifier)
    let status = await AssetInventory.status(forModules: [module])
    if status != .installed {
        fail("The speech assets for \(identifier) are not installed.", code: 2)
    }
}

@available(macOS 26.0, *)
func install(_ identifier: String) async {
    let module = await transcriber(for: identifier)
    do {
        if let request = try await AssetInventory.assetInstallationRequest(supporting: [module]) {
            try await request.downloadAndInstall()
        }
    } catch {
        fail("Could not download the speech assets: \(error.localizedDescription)")
    }
}

@available(macOS 26.0, *)
func stream(_ identifier: String) async {
    let module = await transcriber(for: identifier)
    if await AssetInventory.status(forModules: [module]) != .installed {
        fail("The speech assets for \(identifier) are not installed.", code: 2)
    }
    let source = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 16_000, channels: 1, interleaved: false)!
    guard let target = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: [module]) else {
        fail("No audio format for Apple Speech.", code: 2)
    }
    let converter = source == target ? nil : AVAudioConverter(from: source, to: target)
    let analyzer = SpeechAnalyzer(modules: [module])
    let (inputs, feed) = AsyncStream<AnalyzerInput>.makeStream()

    let output = Task {
        do {
            for try await result in module.results where result.isFinal {
                let text = String(result.text.characters).trimmingCharacters(in: .whitespacesAndNewlines)
                if text.isEmpty { continue }
                let line: [String: Any] = [
                    "start": result.range.start.seconds,
                    "end": result.range.end.seconds,
                    "text": text,
                ]
                let data = try JSONSerialization.data(withJSONObject: line)
                FileHandle.standardOutput.write(data + Data([0x0A]))
            }
        } catch {
            FileHandle.standardError.write("results ended: \(error)\n".data(using: .utf8)!)
        }
    }

    do {
        try await analyzer.start(inputSequence: inputs)
    } catch {
        fail("Apple Speech did not start: \(error.localizedDescription)", code: 2)
    }

    // stdin blocks, so it is read on its own thread. 0.5 s of audio per read.
    let reader = Thread {
        var pending = Data()
        while true {
            let chunk = FileHandle.standardInput.readData(ofLength: 32_000)
            if chunk.isEmpty { break }
            pending.append(chunk)
            let usable = pending.count - pending.count % 4
            if usable == 0 { continue }
            let frames = AVAudioFrameCount(usable / 4)
            guard let buffer = AVAudioPCMBuffer(pcmFormat: source, frameCapacity: frames) else { break }
            buffer.frameLength = frames
            pending.prefix(usable).withUnsafeBytes { raw in
                buffer.floatChannelData![0].update(from: raw.bindMemory(to: Float.self).baseAddress!, count: Int(frames))
            }
            pending.removeFirst(usable)
            if let converter {
                let ratio = target.sampleRate / source.sampleRate
                let capacity = AVAudioFrameCount(Double(frames) * ratio) + 32
                guard let converted = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else { break }
                var given = false
                var error: NSError?
                converter.convert(to: converted, error: &error) { _, status in
                    if given {
                        status.pointee = .noDataNow
                        return nil
                    }
                    given = true
                    status.pointee = .haveData
                    return buffer
                }
                if error == nil, converted.frameLength > 0 {
                    feed.yield(AnalyzerInput(buffer: converted))
                }
            } else {
                feed.yield(AnalyzerInput(buffer: buffer))
            }
        }
        feed.finish()
    }
    reader.start()

    // The input ends when Margin closes stdin at Stop.
    while !reader.isFinished {
        try? await Task.sleep(for: .milliseconds(100))
    }
    do {
        try await analyzer.finalizeAndFinishThroughEndOfInput()
    } catch {
        FileHandle.standardError.write("finalize: \(error)\n".data(using: .utf8)!)
    }
    _ = await output.value
}

@main
struct MarginSpeech {
    static func main() async {
        let args = CommandLine.arguments
        guard args.count == 3 else {
            fail("usage: margin-speech check|install|stream <locale>")
        }
        guard #available(macOS 26.0, *) else {
            fail("Apple Speech needs macOS 26 or later.", code: 2)
        }
        switch args[1] {
        case "check": await check(args[2])
        case "install": await install(args[2])
        case "stream": await stream(args[2])
        default: fail("unknown command \(args[1])")
        }
    }
}
