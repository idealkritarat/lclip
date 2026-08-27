import AppKit
import SwiftUI

final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private let store = AppStore()
    private var statusItem: NSStatusItem?
    private var panel: NSPanel?
    private var eventMonitor: Any?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)

        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        item.button?.title = "LCP"
        item.button?.target = self
        item.button?.action = #selector(togglePanel)
        statusItem = item

        Task { await store.start() }
    }

    @objc private func togglePanel() {
        if panel?.isVisible == true {
            closePanel()
        } else {
            showPanel()
        }
    }

    private func showPanel() {
        if panel == nil {
            let content = ContentView(store: store) { [weak self] in
                self?.closePanel()
            }
            let panel = NSPanel(
                contentRect: NSRect(x: 0, y: 0, width: 420, height: 560),
                styleMask: [.nonactivatingPanel, .fullSizeContentView],
                backing: .buffered,
                defer: false
            )
            panel.contentView = NSHostingView(rootView: content)
            panel.isReleasedWhenClosed = false
            panel.level = .statusBar
            panel.collectionBehavior = [.canJoinAllSpaces, .transient]
            panel.hidesOnDeactivate = true
            panel.delegate = self
            self.panel = panel
        }

        guard let button = statusItem?.button, let panel else { return }
        let buttonFrame = button.window?.convertToScreen(button.frame) ?? .zero
        let size = panel.frame.size
        let origin = NSPoint(
            x: buttonFrame.midX - size.width / 2,
            y: buttonFrame.minY - size.height - 8
        )
        panel.setFrameOrigin(origin)
        panel.orderFrontRegardless()

        eventMonitor = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [weak self] _ in
            self?.closePanel()
        }
    }

    private func closePanel() {
        panel?.orderOut(nil)
        if let eventMonitor {
            NSEvent.removeMonitor(eventMonitor)
            self.eventMonitor = nil
        }
    }

    func windowDidResignKey(_ notification: Notification) {
        closePanel()
    }
}
