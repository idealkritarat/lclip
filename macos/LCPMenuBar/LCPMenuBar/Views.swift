import SwiftUI

struct ContentView: View {
    @ObservedObject var store: AppStore
    let close: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            switch store.route {
            case .friends:
                FriendListView(store: store)
            case .conversation(let peer):
                ConversationView(store: store, peer: peer, close: close)
            }
        }
        .frame(width: 420, height: 560)
        .background(Color(nsColor: .windowBackgroundColor))
    }

    private var header: some View {
        HStack {
            Text("LCP")
                .font(.headline)
            Spacer()
            statusView
            Button(action: close) {
                Image(systemName: "xmark")
            }
            .buttonStyle(.borderless)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    private var statusView: some View {
        Group {
            switch store.daemonStatus {
            case .connecting:
                Text("Connecting")
            case .ready:
                Text("Ready")
            case .unavailable:
                Text("Offline")
            }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
    }
}

struct FriendListView: View {
    @ObservedObject var store: AppStore

    var body: some View {
        if store.peers.isEmpty {
            VStack(spacing: 8) {
                Text("No paired peers")
                    .font(.headline)
                Text("Use `lcp invite` and `lcp pair <ticket>` to pair.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            List(store.peers) { peer in
                HStack(spacing: 10) {
                    Button {
                        store.openConversation(peer)
                    } label: {
                        VStack(alignment: .leading, spacing: 3) {
                            HStack {
                                Circle()
                                    .fill(peer.isOnline ? Color.green : Color.gray)
                                    .frame(width: 8, height: 8)
                                Text(peer.alias)
                                    .font(.body.weight(.semibold))
                            }
                            Text(peer.latestPreview ?? peer.deviceName)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }
                    .buttonStyle(.plain)

                    Spacer()

                    Button {
                        Task { await store.sendClipboard(to: peer) }
                    } label: {
                        Image(systemName: "paperplane")
                    }
                    .disabled(!peer.isOnline)
                    .help("Send clipboard")

                    Button {
                        Task { await store.copyLatest(from: peer) }
                    } label: {
                        Image(systemName: "doc.on.clipboard")
                    }
                    .disabled(peer.latestPreview == nil)
                    .help("Copy latest")
                }
                .padding(.vertical, 5)
            }
            .listStyle(.inset)
        }
    }
}

struct ConversationView: View {
    @ObservedObject var store: AppStore
    let peer: PeerViewModel
    let close: () -> Void

    var messages: [MessageViewModel] {
        store.conversations[peer.endpointID] ?? []
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Button {
                    store.backToFriends()
                } label: {
                    Image(systemName: "chevron.left")
                }
                .buttonStyle(.borderless)
                Text(peer.alias)
                    .font(.headline)
                Spacer()
                Button {
                    Task { await store.sendClipboard(to: peer) }
                } label: {
                    Image(systemName: "paperplane")
                }
                .disabled(!peer.isOnline)
                .help("Send clipboard")
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)

            Divider()

            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        ForEach(messages) { message in
                            MessageRow(message: message) {
                                store.copy(message)
                            } retry: {
                                Task { await store.retry(message) }
                            }
                            .id(message.id)
                        }
                    }
                    .padding(14)
                }
                .onChange(of: messages.count) { _ in
                    if let id = messages.last?.id {
                        proxy.scrollTo(id, anchor: .bottom)
                    }
                }
            }

            Divider()

            HStack(alignment: .bottom, spacing: 8) {
                TextEditor(text: $store.composerText)
                    .font(.system(.body, design: .monospaced))
                    .frame(minHeight: 52, maxHeight: 96)
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.secondary.opacity(0.25)))
                Button {
                    Task { await store.sendComposer(to: peer) }
                } label: {
                    Image(systemName: "arrow.up.circle.fill")
                        .font(.title2)
                }
                .buttonStyle(.borderless)
                .disabled(!peer.isOnline || store.composerText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                .keyboardShortcut(.return, modifiers: [])
            }
            .padding(12)
        }
        .onExitCommand {
            store.backToFriends()
        }
    }
}

struct MessageRow: View {
    let message: MessageViewModel
    let copy: () -> Void
    let retry: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack {
                Text("\(message.senderLabel) · \(message.receivedAt.formatted(date: .omitted, time: .shortened))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                if message.isFailed {
                    Button("Retry", action: retry)
                        .font(.caption)
                } else {
                    Text(message.status)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            Text(message.text)
                .font(.system(.body, design: .monospaced))
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .onTapGesture(perform: copy)
        }
    }
}
