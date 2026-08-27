import Foundation

enum DaemonStatus: Equatable {
    case connecting
    case ready
    case unavailable(String)
}

enum Route: Equatable {
    case friends
    case conversation(PeerViewModel)
}

struct PeerViewModel: Identifiable, Equatable {
    let endpointID: String
    var alias: String
    var deviceName: String
    var status: String
    var path: String?
    var latestPreview: String?
    var latestAt: Date?

    var id: String { endpointID }
    var isOnline: Bool { status == "online" }
}

struct MessageViewModel: Identifiable, Equatable {
    let id: String
    let peerID: String
    let direction: String
    let senderLabel: String
    let text: String
    let sentAt: Date
    let receivedAt: Date
    let status: String

    var isFailed: Bool { status == "failed" }
}
