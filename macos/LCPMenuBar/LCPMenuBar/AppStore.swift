import AppKit
import Foundation

@MainActor
final class AppStore: ObservableObject {
    @Published var peers: [PeerViewModel] = []
    @Published var conversations: [String: [MessageViewModel]] = [:]
    @Published var route: Route = .friends
    @Published var daemonStatus: DaemonStatus = .connecting
    @Published var composerText = ""

    private var client: IPCClient?
    private var reconnectTask: Task<Void, Never>?

    func start() async {
        guard reconnectTask == nil else { return }
        reconnectTask = Task { await connectLoop() }
    }

    func openConversation(_ peer: PeerViewModel) {
        route = .conversation(peer)
        Task { await refreshMessages(peerID: peer.endpointID) }
    }

    func backToFriends() {
        route = .friends
        composerText = ""
    }

    func sendClipboard(to peer: PeerViewModel) async {
        guard let text = NSPasteboard.general.string(forType: .string), !text.isEmpty else { return }
        await send(text: text, to: peer)
    }

    func copyLatest(from peer: PeerViewModel) async {
        do {
            let result = try await client?.call(method: "get_latest_incoming", params: ["peer": peer.endpointID])
            guard let text = result?["text"] as? String else { return }
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
        } catch {
            daemonStatus = .unavailable(error.localizedDescription)
        }
    }

    func copy(_ message: MessageViewModel) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(message.text, forType: .string)
    }

    func sendComposer(to peer: PeerViewModel) async {
        let text = composerText
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        composerText = ""
        await send(text: text, to: peer)
    }

    func retry(_ message: MessageViewModel) async {
        guard let peer = peers.first(where: { $0.endpointID == message.peerID }) else { return }
        await send(text: message.text, to: peer)
    }

    private func connectLoop() async {
        while !Task.isCancelled {
            do {
                daemonStatus = .connecting
                let nextClient = try IPCClient()
                nextClient.onEvent = { [weak self] event in
                    Task { @MainActor in
                        await self?.handle(event: event)
                    }
                }
                nextClient.onDisconnect = { [weak self] message in
                    Task { @MainActor in
                        self?.client = nil
                        self?.daemonStatus = .unavailable(message)
                        self?.reconnectTask = nil
                        await self?.start()
                    }
                }
                client = nextClient
                _ = try await nextClient.call(method: "hello")
                await refreshSnapshot()
                _ = try await nextClient.call(method: "subscribe")
                daemonStatus = .ready
                return
            } catch {
                daemonStatus = .unavailable(error.localizedDescription)
                try? await Task.sleep(nanoseconds: 1_500_000_000)
            }
        }
    }

    private func send(text: String, to peer: PeerViewModel) async {
        do {
            _ = try await client?.call(method: "send_text", params: ["peer": peer.endpointID, "text": text])
            await refreshMessages(peerID: peer.endpointID)
        } catch {
            await refreshMessages(peerID: peer.endpointID)
        }
    }

    private func refreshSnapshot() async {
        await refreshPeers()
        await refreshMessages(peerID: nil)
    }

    private func refreshPeers() async {
        do {
            let result = try await client?.call(method: "list_peers")
            let rows = (result?["value"] as? [[String: Any]]) ?? []
            peers = rows.map(parsePeer).sorted { $0.alias.localizedCaseInsensitiveCompare($1.alias) == .orderedAscending }
            applyLatestPreviews()
        } catch {
            daemonStatus = .unavailable(error.localizedDescription)
        }
    }

    private func refreshMessages(peerID: String?) async {
        do {
            var params: [String: Any] = [:]
            if let peerID { params["peer"] = peerID }
            let result = try await client?.call(method: "list_messages", params: params)
            let rows = (result?["value"] as? [[String: Any]]) ?? []
            let messages = rows.map(parseMessage).sorted { $0.receivedAt < $1.receivedAt }
            if let peerID {
                conversations[peerID] = messages
            } else {
                conversations = Dictionary(grouping: messages, by: { $0.peerID })
            }
            applyLatestPreviews()
        } catch {
            daemonStatus = .unavailable(error.localizedDescription)
        }
    }

    private func handle(event: [String: Any]) async {
        guard let name = event["event"] as? String else { return }
        switch name {
        case "peer_updated":
            await refreshPeers()
        case "message_received", "message_updated":
            await refreshMessages(peerID: currentPeerID())
            if currentPeerID() == nil {
                await refreshMessages(peerID: nil)
            }
        default:
            break
        }
    }

    private func currentPeerID() -> String? {
        if case .conversation(let peer) = route {
            return peer.endpointID
        }
        return nil
    }

    private func applyLatestPreviews() {
        peers = peers.map { peer in
            var next = peer
            if let latest = conversations[peer.endpointID]?.filter({ $0.direction == "incoming" }).last {
                next.latestPreview = latest.text.replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
                next.latestAt = latest.receivedAt
            }
            return next
        }
    }

    private func parsePeer(_ row: [String: Any]) -> PeerViewModel {
        PeerViewModel(
            endpointID: row["endpoint_id"] as? String ?? "",
            alias: row["alias"] as? String ?? "Unknown",
            deviceName: row["device_name"] as? String ?? "",
            status: row["status"] as? String ?? "offline",
            path: row["path"] as? String
        )
    }

    private func parseMessage(_ row: [String: Any]) -> MessageViewModel {
        MessageViewModel(
            id: row["message_id"] as? String ?? UUID().uuidString,
            peerID: row["peer_id"] as? String ?? "",
            direction: row["direction"] as? String ?? "incoming",
            senderLabel: row["sender_label"] as? String ?? "",
            text: row["text"] as? String ?? "",
            sentAt: date(ms: row["sent_at_unix_ms"]),
            receivedAt: date(ms: row["received_at_unix_ms"]),
            status: row["status"] as? String ?? "received"
        )
    }

    private func date(ms: Any?) -> Date {
        if let value = ms as? Double {
            return Date(timeIntervalSince1970: value / 1000)
        }
        if let value = ms as? Int {
            return Date(timeIntervalSince1970: Double(value) / 1000)
        }
        return Date()
    }
}
