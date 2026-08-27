import Foundation
import Darwin

enum IPCError: Error, LocalizedError {
    case connectFailed(String)
    case disconnected
    case daemonError(String)
    case malformedFrame

    var errorDescription: String? {
        switch self {
        case .connectFailed(let message): return message
        case .disconnected: return "lanclipd disconnected"
        case .daemonError(let message): return message
        case .malformedFrame: return "lanclipd sent a malformed IPC frame"
        }
    }
}

final class IPCClient {
    static let ipcVersion = 1

    private let fd: Int32
    private let queue = DispatchQueue(label: "lcp.ipc")
    private var pending: [String: (Result<[String: Any], Error>) -> Void] = [:]
    var onEvent: (([String: Any]) -> Void)?
    var onDisconnect: ((String) -> Void)?

    init(socketPath: String = IPCClient.defaultSocketPath()) throws {
        fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {
            throw IPCError.connectFailed("could not create unix socket")
        }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(socketPath.utf8CString)
        guard pathBytes.count <= MemoryLayout.size(ofValue: addr.sun_path) else {
            close(fd)
            throw IPCError.connectFailed("socket path is too long")
        }

        withUnsafeMutableBytes(of: &addr.sun_path) { raw in
            for i in 0..<pathBytes.count {
                raw[i] = UInt8(bitPattern: pathBytes[i])
            }
        }

        let len = socklen_t(MemoryLayout<sa_family_t>.size + pathBytes.count)
        let result = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(fd, $0, len)
            }
        }

        guard result == 0 else {
            let message = String(cString: strerror(errno))
            close(fd)
            throw IPCError.connectFailed("could not connect to lanclipd: \(message)")
        }

        queue.async { [weak self] in
            self?.readLoop()
        }
    }

    deinit {
        close(fd)
    }

    static func defaultSocketPath() -> String {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        return "\(home)/Library/Application Support/lcp/lanclipd.sock"
    }

    func call(method: String, params: [String: Any] = [:]) async throws -> [String: Any] {
        try await withCheckedThrowingContinuation { continuation in
            queue.async {
                let id = UUID().uuidString.lowercased()
                self.pending[id] = { result in
                    continuation.resume(with: result)
                }

                let request: [String: Any] = [
                    "ipc_version": Self.ipcVersion,
                    "id": id,
                    "method": method,
                    "params": params
                ]

                do {
                    let body = try JSONSerialization.data(withJSONObject: request)
                    try self.writeFrame(body)
                } catch {
                    self.pending.removeValue(forKey: id)
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    private func readLoop() {
        while true {
            do {
                let body = try readFrame()
                guard let frame = try JSONSerialization.jsonObject(with: body) as? [String: Any] else {
                    throw IPCError.malformedFrame
                }

                if let event = frame["event"] as? String {
                    DispatchQueue.main.async { [weak self] in
                        var eventFrame = frame
                        eventFrame["event"] = event
                        self?.onEvent?(eventFrame)
                    }
                    continue
                }

                guard let id = frame["id"] as? String else {
                    throw IPCError.malformedFrame
                }

                let callback = pending.removeValue(forKey: id)
                if frame["ok"] as? Bool == true {
                    let result = frame["result"] as? [String: Any] ?? ["value": frame["result"] as Any]
                    callback?(.success(result))
                } else {
                    let error = frame["error"] as? [String: Any]
                    let message = error?["message"] as? String ?? "daemon request failed"
                    callback?(.failure(IPCError.daemonError(message)))
                }
            } catch {
                let callbacks = pending.values
                pending.removeAll()
                callbacks.forEach { $0(.failure(error)) }
                DispatchQueue.main.async { [weak self] in
                    self?.onDisconnect?(error.localizedDescription)
                }
                break
            }
        }
    }

    private func readFrame() throws -> Data {
        let header = try readExactly(4)
        let length = header.reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
        guard length > 0, length <= 6 * 1024 * 1024 else {
            throw IPCError.malformedFrame
        }
        return try readExactly(Int(length))
    }

    private func readExactly(_ count: Int) throws -> Data {
        var data = Data()
        data.reserveCapacity(count)

        while data.count < count {
            var buffer = [UInt8](repeating: 0, count: count - data.count)
            let n = buffer.withUnsafeMutableBytes { raw in
                Darwin.read(fd, raw.baseAddress, raw.count)
            }
            if n == 0 { throw IPCError.disconnected }
            if n < 0 {
                if errno == EINTR { continue }
                throw IPCError.disconnected
            }
            data.append(buffer, count: n)
        }

        return data
    }

    private func writeFrame(_ body: Data) throws {
        var length = UInt32(body.count).bigEndian
        var frame = Data(bytes: &length, count: 4)
        frame.append(body)
        try frame.withUnsafeBytes { raw in
            guard let base = raw.baseAddress else { return }
            var offset = 0
            while offset < frame.count {
                let n = Darwin.write(fd, base.advanced(by: offset), frame.count - offset)
                if n < 0 {
                    if errno == EINTR { continue }
                    throw IPCError.disconnected
                }
                offset += n
            }
        }
    }
}
