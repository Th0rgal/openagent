//
//  ControlSessionManager.swift
//  OpenAgentDashboard
//
//  Manages the SSE stream connection for the control session.
//  Handles auto-reconnection, state recovery, and graceful error handling.
//

import Foundation
import Observation

@MainActor
@Observable
final class ControlSessionManager {
    static let shared = ControlSessionManager()
    
    // MARK: - State
    
    var messages: [ChatMessage] = []
    var runState: ControlRunState = .idle
    var queueLength: Int = 0
    var currentMission: Mission?
    var isLoading: Bool = false
    
    // MARK: - Private
    
    private var streamTask: Task<Void, Never>?
    private var reconnectTask: Task<Void, Never>?
    private var isConnected = false
    private var reconnectAttempts = 0
    private let maxReconnectDelay: TimeInterval = 30
    private let api = APIService.shared
    
    private init() {}
    
    // MARK: - Public API
    
    /// Start the streaming connection. Call once on app launch.
    func start() {
        guard streamTask == nil else { return }
        connect()
    }
    
    /// Stop the streaming connection.
    func stop() {
        streamTask?.cancel()
        streamTask = nil
        reconnectTask?.cancel()
        reconnectTask = nil
        isConnected = false
    }
    
    /// Load a specific mission by ID.
    func loadMission(id: String) async {
        isLoading = true
        defer { isLoading = false }
        
        do {
            let missions = try await api.listMissions()
            if let mission = missions.first(where: { $0.id == id }) {
                switchToMission(mission)
                HapticService.success()
            }
        } catch {
            print("Failed to load mission \(id): \(error)")
        }
    }
    
    /// Load the current mission from the server.
    func loadCurrentMission() async {
        isLoading = true
        defer { isLoading = false }
        
        do {
            if let mission = try await api.getCurrentMission() {
                switchToMission(mission)
            }
        } catch {
            print("Failed to load current mission: \(error)")
        }
    }
    
    /// Create a new mission.
    func createNewMission() async {
        do {
            let mission = try await api.createMission()
            currentMission = mission
            messages = []
            HapticService.success()
        } catch {
            print("Failed to create mission: \(error)")
            HapticService.error()
        }
    }
    
    /// Set mission status.
    func setMissionStatus(_ status: MissionStatus) async {
        guard let mission = currentMission else { return }
        
        do {
            try await api.setMissionStatus(id: mission.id, status: status)
            currentMission?.status = status
            HapticService.success()
        } catch {
            print("Failed to set status: \(error)")
            HapticService.error()
        }
    }
    
    /// Send a message to the agent.
    func sendMessage(content: String) async {
        let trimmed = content.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        
        HapticService.lightTap()
        
        do {
            let _ = try await api.sendMessage(content: trimmed)
        } catch {
            print("Failed to send message: \(error)")
            HapticService.error()
        }
    }
    
    /// Cancel the current run.
    func cancelRun() async {
        do {
            try await api.cancelControl()
            HapticService.success()
        } catch {
            print("Failed to cancel: \(error)")
            HapticService.error()
        }
    }
    
    // MARK: - Private Helpers
    
    private func switchToMission(_ mission: Mission) {
        currentMission = mission
        messages = mission.history.enumerated().map { index, entry in
            ChatMessage(
                id: "\(mission.id)-\(index)",
                type: entry.isUser ? .user : .assistant(success: true, costCents: 0, model: nil),
                content: entry.content
            )
        }
    }
    
    private func connect() {
        streamTask = Task { [weak self] in
            guard let self = self else { return }
            
            guard let url = URL(string: "\(api.baseURL)/api/control/stream") else {
                scheduleReconnect()
                return
            }
            
            var request = URLRequest(url: url)
            request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
            
            if api.isAuthenticated, let token = UserDefaults.standard.string(forKey: "jwt_token") {
                request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
            }
            
            do {
                let (stream, response) = try await URLSession.shared.bytes(for: request)
                
                // Check for HTTP errors
                if let httpResponse = response as? HTTPURLResponse {
                    guard (200...299).contains(httpResponse.statusCode) else {
                        scheduleReconnect()
                        return
                    }
                }
                
                // Successfully connected
                isConnected = true
                reconnectAttempts = 0
                
                // Sync mission state on reconnect
                await syncMissionState()
                
                var buffer = ""
                for try await byte in stream {
                    guard !Task.isCancelled else { break }
                    
                    if let char = String(bytes: [byte], encoding: .utf8) {
                        buffer.append(char)
                        
                        // Look for double newline (end of SSE event)
                        while let range = buffer.range(of: "\n\n") {
                            let eventString = String(buffer[..<range.lowerBound])
                            buffer = String(buffer[range.upperBound...])
                            
                            parseAndHandleEvent(eventString)
                        }
                    }
                }
                
                // Stream ended normally or was cancelled
                isConnected = false
                if !Task.isCancelled {
                    scheduleReconnect()
                }
            } catch {
                isConnected = false
                if !Task.isCancelled {
                    // Don't show transient stream errors to users
                    print("Stream error: \(error.localizedDescription)")
                    scheduleReconnect()
                }
            }
        }
    }
    
    private func scheduleReconnect() {
        guard !Task.isCancelled else { return }
        
        reconnectTask?.cancel()
        reconnectTask = Task { [weak self] in
            guard let self = self else { return }
            
            // Exponential backoff: 1s, 2s, 4s, 8s, ... up to maxReconnectDelay
            let delay = min(pow(2.0, Double(reconnectAttempts)), maxReconnectDelay)
            reconnectAttempts += 1
            
            try? await Task.sleep(nanoseconds: UInt64(delay * 1_000_000_000))
            
            guard !Task.isCancelled else { return }
            
            streamTask = nil
            connect()
        }
    }
    
    /// Sync mission state after reconnecting to recover any missed messages.
    private func syncMissionState() async {
        guard let mission = currentMission else {
            // No mission loaded, try to load current
            await loadCurrentMission()
            return
        }
        
        // Refresh the current mission to get any messages we missed
        do {
            if let refreshed = try await api.getCurrentMission(), refreshed.id == mission.id {
                // Only update messages if the server has more than we do locally
                // This preserves any streaming messages we're currently receiving
                let serverMessageCount = refreshed.history.count
                let localPersistentCount = messages.filter { !$0.isThinking }.count
                
                if serverMessageCount > localPersistentCount {
                    switchToMission(refreshed)
                }
            }
        } catch {
            print("Failed to sync mission state: \(error)")
        }
    }
    
    private func parseAndHandleEvent(_ eventString: String) {
        var eventType = "message"
        var dataLines: [String] = []
        
        for line in eventString.split(separator: "\n", omittingEmptySubsequences: false) {
            let lineStr = String(line)
            if lineStr.hasPrefix("event:") {
                eventType = String(lineStr.dropFirst(6)).trimmingCharacters(in: .whitespaces)
            } else if lineStr.hasPrefix("data:") {
                dataLines.append(String(lineStr.dropFirst(5)).trimmingCharacters(in: .whitespaces))
            }
            // Ignore SSE comments (lines starting with :)
        }
        
        let dataString = dataLines.joined()
        guard !dataString.isEmpty else { return }
        
        do {
            guard let data = dataString.data(using: .utf8),
                  let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                return
            }
            handleEvent(type: eventType, data: json)
        } catch {
            // Silently ignore parse errors - could be partial data or keepalive
        }
    }
    
    private func handleEvent(type: String, data: [String: Any]) {
        switch type {
        case "status":
            if let state = data["state"] as? String {
                runState = ControlRunState(rawValue: state) ?? .idle
            }
            if let queue = data["queue_len"] as? Int {
                queueLength = queue
            }
            
        case "user_message":
            if let content = data["content"] as? String,
               let id = data["id"] as? String {
                // Avoid duplicates (might already have this from mission history)
                if !messages.contains(where: { $0.id == id }) {
                    let message = ChatMessage(id: id, type: .user, content: content)
                    messages.append(message)
                }
            }
            
        case "assistant_message":
            if let content = data["content"] as? String,
               let id = data["id"] as? String {
                let success = data["success"] as? Bool ?? true
                let costCents = data["cost_cents"] as? Int ?? 0
                let model = data["model"] as? String
                
                // Remove any incomplete thinking messages
                messages.removeAll { $0.isThinking && !$0.thinkingDone }
                
                // Avoid duplicates
                if !messages.contains(where: { $0.id == id }) {
                    let message = ChatMessage(
                        id: id,
                        type: .assistant(success: success, costCents: costCents, model: model),
                        content: content
                    )
                    messages.append(message)
                }
            }
            
        case "thinking":
            if let content = data["content"] as? String {
                let done = data["done"] as? Bool ?? false
                
                // Find existing thinking message or create new
                if let index = messages.lastIndex(where: { $0.isThinking && !$0.thinkingDone }) {
                    messages[index].content += "\n\n---\n\n" + content
                    if done {
                        messages[index] = ChatMessage(
                            id: messages[index].id,
                            type: .thinking(done: true, startTime: Date()),
                            content: messages[index].content
                        )
                    }
                } else if !done {
                    let message = ChatMessage(
                        id: "thinking-\(Date().timeIntervalSince1970)",
                        type: .thinking(done: false, startTime: Date()),
                        content: content
                    )
                    messages.append(message)
                }
            }
            
        case "error":
            // Only show actual agent errors, not stream connection errors
            if let errorMessage = data["message"] as? String,
               !errorMessage.contains("Stream connection") {
                let message = ChatMessage(
                    id: "error-\(Date().timeIntervalSince1970)",
                    type: .error,
                    content: errorMessage
                )
                messages.append(message)
            }
            
        case "tool_call":
            if let toolCallId = data["tool_call_id"] as? String,
               let name = data["name"] as? String,
               let args = data["args"] as? [String: Any] {
                // Parse UI tool calls
                if let toolUI = ToolUIContent.parse(name: name, args: args) {
                    let message = ChatMessage(
                        id: toolCallId,
                        type: .toolUI(name: name),
                        content: "",
                        toolUI: toolUI
                    )
                    messages.append(message)
                }
            }
            
        default:
            break
        }
    }
}
