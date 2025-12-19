#!/bin/bash
# Model Comparison Test Script
# Tests open_agent's model performance on a security research task
#
# Usage:
#   ./test_model_comparison.sh [API_URL]
#   
#   API_URL: Backend API URL (default: https://agent-backend.thomas.md)
#
# Environment:
#   DASHBOARD_PASSWORD: Required for auth

set -e

API_URL="${1:-https://agent-backend.thomas.md}"
RESULTS_DIR="$(dirname "$0")/../test_results/model_comparison_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$RESULTS_DIR"

echo "==========================================="
echo "Model Comparison Test"
echo "API: $API_URL"
echo "Results: $RESULTS_DIR"
echo "==========================================="

# Models to test
MODELS=(
    "moonshotai/kimi-k2-thinking"
    "x-ai/grok-4.1-fast"
    "google/gemini-3-flash-preview"
    "deepseek/deepseek-v3.2-speciale"
    "qwen/qwen3-vl-235b-a22b-thinking"
    "mistralai/mistral-large-2512"
    "amazon/nova-pro-v1"
    "z-ai/glm-4.6v"
    # Baselines
    "anthropic/claude-sonnet-4.5"
    "google/gemini-2.5-pro"
)

# The security analysis task
TASK_DESCRIPTION='Download Rabby Wallet extension for Chrome, decompile it, and look for security vulnerabilities similar to the Permit2 transaction simulation bypass bug. Focus on:

1. How Rabby parses and validates Permit2 signatures
2. Whether the spender field is properly validated against known contract addresses
3. If the witness data can be manipulated to display incorrect transaction details
4. Any other transaction simulation bypass vectors

Provide findings in a structured markdown report with:
- Vulnerability title
- Severity (Critical/High/Medium/Low)
- Description
- Proof of concept outline
- Recommended fix'

# Authenticate
echo ""
echo "[Auth] Getting JWT token..."
if [ -z "$DASHBOARD_PASSWORD" ]; then
    echo "Warning: DASHBOARD_PASSWORD not set, trying without auth (DEV_MODE)"
    AUTH_HEADER=""
else
    TOKEN_RESPONSE=$(curl -s -X POST "$API_URL/api/auth/login" \
        -H "Content-Type: application/json" \
        -d "{\"password\": \"$DASHBOARD_PASSWORD\"}")
    
    TOKEN=$(echo "$TOKEN_RESPONSE" | jq -r '.token // empty')
    if [ -z "$TOKEN" ]; then
        echo "Failed to get token: $TOKEN_RESPONSE"
        echo "Trying without auth..."
        AUTH_HEADER=""
    else
        AUTH_HEADER="Authorization: Bearer $TOKEN"
        echo "Got token: ${TOKEN:0:20}..."
    fi
fi

# Function to submit task and wait for completion
submit_and_wait() {
    local model="$1"
    local task="$2"
    local result_file="$3"
    local timeout_seconds=600  # 10 min timeout per model
    
    echo ""
    echo "==========================================="
    echo "Testing: $model"
    echo "==========================================="
    
    local start_time=$(date +%s)
    
    # Submit task
    local create_payload=$(jq -n \
        --arg task "$task" \
        --arg model "$model" \
        '{task: $task, model: $model}')
    
    local create_response
    if [ -n "$AUTH_HEADER" ]; then
        create_response=$(curl -s -X POST "$API_URL/api/task" \
            -H "Content-Type: application/json" \
            -H "$AUTH_HEADER" \
            -d "$create_payload")
    else
        create_response=$(curl -s -X POST "$API_URL/api/task" \
            -H "Content-Type: application/json" \
            -d "$create_payload")
    fi
    
    local task_id=$(echo "$create_response" | jq -r '.id // empty')
    
    if [ -z "$task_id" ]; then
        echo "Failed to create task: $create_response"
        echo "{\"model\": \"$model\", \"error\": \"failed to create task\", \"response\": $create_response}" > "$result_file"
        return 1
    fi
    
    echo "Task ID: $task_id"
    echo "Waiting for completion..."
    
    # Poll for completion
    local status="pending"
    local poll_count=0
    local max_polls=$((timeout_seconds / 5))
    
    while [ "$status" != "completed" ] && [ "$status" != "failed" ] && [ $poll_count -lt $max_polls ]; do
        sleep 5
        poll_count=$((poll_count + 1))
        
        local status_response
        if [ -n "$AUTH_HEADER" ]; then
            status_response=$(curl -s "$API_URL/api/task/$task_id" -H "$AUTH_HEADER")
        else
            status_response=$(curl -s "$API_URL/api/task/$task_id")
        fi
        
        status=$(echo "$status_response" | jq -r '.status // "unknown"')
        echo "  Status: $status (poll $poll_count/$max_polls)"
        
        # Save intermediate status
        echo "$status_response" > "${result_file%.json}_latest.json"
    done
    
    local end_time=$(date +%s)
    local duration=$((end_time - start_time))
    
    # Get final result
    local final_response
    if [ -n "$AUTH_HEADER" ]; then
        final_response=$(curl -s "$API_URL/api/task/$task_id" -H "$AUTH_HEADER")
    else
        final_response=$(curl -s "$API_URL/api/task/$task_id")
    fi
    
    # Extract metrics
    local final_status=$(echo "$final_response" | jq -r '.status // "unknown"')
    local cost_cents=$(echo "$final_response" | jq -r '.cost_cents // 0')
    local result=$(echo "$final_response" | jq -r '.result // ""')
    local result_length=${#result}
    
    # Build summary
    local summary=$(jq -n \
        --arg model "$model" \
        --arg task_id "$task_id" \
        --arg status "$final_status" \
        --argjson duration "$duration" \
        --argjson cost_cents "$cost_cents" \
        --argjson result_length "$result_length" \
        --argjson full_response "$final_response" \
        '{
            model: $model,
            task_id: $task_id,
            status: $status,
            duration_seconds: $duration,
            cost_cents: $cost_cents,
            result_length: $result_length,
            full_response: $full_response
        }')
    
    echo "$summary" > "$result_file"
    
    echo ""
    echo "Results for $model:"
    echo "  Status: $final_status"
    echo "  Duration: ${duration}s"
    echo "  Cost: $cost_cents cents"
    echo "  Result length: $result_length chars"
    
    return 0
}

# Summary file
SUMMARY_FILE="$RESULTS_DIR/summary.json"
echo "[]" > "$SUMMARY_FILE"

# Test each model
for model in "${MODELS[@]}"; do
    safe_name=$(echo "$model" | tr '/' '_' | tr ':' '_')
    result_file="$RESULTS_DIR/${safe_name}.json"
    
    if submit_and_wait "$model" "$TASK_DESCRIPTION" "$result_file"; then
        # Append to summary
        jq -s '.[0] + [.[1]]' "$SUMMARY_FILE" <(jq '{model, status, duration_seconds, cost_cents, result_length}' "$result_file") > "${SUMMARY_FILE}.tmp"
        mv "${SUMMARY_FILE}.tmp" "$SUMMARY_FILE"
    fi
    
    # Small delay between models
    sleep 2
done

echo ""
echo "==========================================="
echo "Test Complete!"
echo "==========================================="
echo ""
echo "Summary:"
jq -r '.[] | "\(.model): \(.status) in \(.duration_seconds)s, \(.cost_cents) cents, \(.result_length) chars"' "$SUMMARY_FILE"

echo ""
echo "Full results saved to: $RESULTS_DIR"
