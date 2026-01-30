# iOS Dashboard Issues - Comparison with Web Dashboard

## Fixed Issues

### 1. Tool Call Display - Missing Arguments and Results ✅ FIXED
**Was:** iOS only shows tool name with a spinner when active, then removes it when done
**Now:** Shows tool name, arguments preview, duration timer, expandable arguments/results, error detection

**Changes:**
- Added `ToolCallData` struct in `ChatMessage.swift` to store tool call data
- Updated `ToolCallBubble` component to show args preview, duration, state indicators
- Expandable view shows full JSON arguments and results

### 2. Tool Calls Are Removed Instead of Preserved ✅ FIXED
**Was:** `messages.removeAll { $0.isToolCall && $0.isActiveToolCall }` - removes previous tool when new one starts
**Now:** Marks previous tool as completed, keeps all tools in history

**Changes:**
- Updated `tool_call` handler to mark previous active tools as completed instead of removing
- Updated `assistant_message` handler to preserve completed tool calls

### 3. Missing Tool Results Display ✅ FIXED
**Was:** `tool_result` event only extracts desktop display ID, ignores actual result content
**Now:** Stores and displays tool results with error detection

**Changes:**
- Updated `tool_result` handler to find matching tool call and update its result
- Added error detection based on result content
- Shows result in expandable section with error highlighting

### 4. No Tool Duration Tracking ✅ FIXED
**Was:** No elapsed time shown for tools
**Now:** Shows "Xs..." with live timer while running, then final duration when done

**Changes:**
- Added `startTime` and `endTime` to `ToolCallData`
- Added timer in `ToolCallBubble` that updates every second
- Shows formatted duration in collapsed and expanded views

### 5. Missing `mission_status_changed` Event Handler ✅ FIXED
**Was:** Not handled
**Now:** Updates mission status, marks pending tools as cancelled when mission ends

**Changes:**
- Added `mission_status_changed` case to `handleStreamEvent`
- Marks all active tools as cancelled when mission ends
- Updates viewing/current mission status
- Triggers refresh of running missions

### 6. No Stall Detection ✅ ALREADY EXISTED
**Was:** Previously implemented in `RunningMissionsBar`
**Now:** Shows warning after 60+ seconds of no activity with amber/red indicators

### 7. Missing Tool Arguments in Event Data ✅ FIXED
**Was:** Tool args parsed but only used for ToolUI parsing
**Now:** Stored in `ToolCallData` and displayed in UI

### 12. Running Missions Bar Missing Queue Info ✅ FIXED
**Was:** Shows state indicator only
**Now:** Also shows queue length indicator (e.g., "2Q")

**Changes:**
- Added queue length display to `runningMissionChip` in `RunningMissionsBar.swift`

### 15. No Error State Distinction in Tools ✅ FIXED
**Was:** No visual distinction for errors
**Now:** Red highlighting for error results, amber for cancelled, green for success

**Changes:**
- Added `ToolCallState` enum with running/success/error/cancelled states
- Color-coded borders, icons, and text based on state

## Remaining Issues (Lower Priority)

### 8. History View Missing Tool Information
When loading mission history, tool calls aren't extracted from events.
Could add parsing of tool_call/tool_result from stored history.

### 9. No Tool Grouping/Collapsing
Dashboard groups consecutive tools with "Show X previous tools" toggle.
Could add grouping logic for cleaner conversation view.

### 10. Missing Image Extraction from Tool Results
Dashboard extracts image paths from screenshot tool results.
`ToolCallData.imagePaths` is implemented but preview not yet displayed.

### 11. No JSON Syntax Highlighting
Dashboard uses syntax highlighter for JSON args/results.
Currently showing plain monospaced text.

### 13. No Copy Functionality for Tool Data
Dashboard allows copying tool args/results.
Could add long-press to copy.

### 14. Missing Model Info in Status Bar During Execution
Dashboard shows current model being used during execution.
Status bar shows "Running" but not which model.
