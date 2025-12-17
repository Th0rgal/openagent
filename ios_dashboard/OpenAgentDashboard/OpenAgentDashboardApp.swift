//
//  OpenAgentDashboardApp.swift
//  OpenAgentDashboard
//
//  iOS Dashboard for Open Agent with liquid glass design
//

import SwiftUI

@main
struct OpenAgentDashboardApp: App {
    init() {
        // Start the control session manager early so it connects immediately
        // and maintains connection across tab switches
        Task { @MainActor in
            ControlSessionManager.shared.start()
        }
    }
    
    var body: some Scene {
        WindowGroup {
            ContentView()
                .preferredColorScheme(.dark)
        }
    }
}
