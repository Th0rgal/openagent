#!/bin/bash
# Quick Model Capability Test
# Tests each model with a simple task to verify basic functionality
#
# Usage:
#   ./quick_model_test.sh [API_URL]

set -e

API_URL="${1:-https://agent-backend.thomas.md}"
RESULTS_DIR="$(dirname "$0")/../test_results/quick_test_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$RESULTS_DIR"

echo "==========================================="
echo "Quick Model Capability Test"
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

# A quick test task that exercises tool usage
TASK='1. Read the file /etc/os-release to identify the OS
2. List the contents of the current working directory
3. Create a simple Python script that prints "Hello from <model>" where <model> is your model name
4. Run the script and capture its output
5. Report back what you found and any observations

Be concise but thorough.'

# Authenticate
echo ""
echo "[Auth] Checking authentication..."
if [ -z "$DASHBOARD_PASSWORD" ]; then
    echo "Warning: DASHBOARD_PASSWORD not set, trying DEV_MODE"
    AUTH_HEADER=""
else
    TOKEN_RESPONSE=$(curl -s -X POST "$API_URL/api/auth/login" \
        -H "Content-Type: application/json" \
        -d "{\"password\": \"$DASHBOARD_PASSWORD\"}")
    
    TOKEN=$(echo "$TOKEN_RESPONSE" | jq -r '.token // empty')
    if [ -z "$TOKEN" ]; then
        echo "Auth failed, trying without: $TOKEN_RESPONSE"
        AUTH_HEADER=""
    else
        AUTH_HEADER="Authorization: Bearer $TOKEN"
        echo "Authenticated"
    fi
fi

# Results array
declare -a RESULTS

# Function to test a model
test_model() {
    local model="$1"
    local timeout_seconds=180  # 3 min timeout for quick test
    
    echo ""
    echo "-------------------------------------------"
    echo "Testing: $model"
    echo "-------------------------------------------"
    
    local start_time=$(date +%s)
    local safe_name=$(echo "$model" | tr '/' '_' | tr ':' '_')
    
    # Submit task
    local create_payload=$(jq -n \
        --arg task "$TASK" \
        --arg model "$model" \
        '{task: $task, model: $model}')
    
    local create_response
    if [ -n "$AUTH_HEADER" ]; then
        create_response=$(curl -s -X POST "$API_URL/api/task" \
            -H "Content-Type: application/json" \
            -H "$AUTH_HEADER" \
            -d "$create_payload" 2>&1)
    else
        create_response=$(curl -s -X POST "$API_URL/api/task" \
            -H "Content-Type: application/json" \
            -d "$create_payload" 2>&1)
    fi
    
    local task_id=$(echo "$create_response" | jq -r '.id // empty' 2>/dev/null)
    
    if [ -z "$task_id" ]; then
        echo "  FAILED to create task: $create_response"
        RESULTS+=("$model|FAILED|0|0|create_error")
        return 1
    fi
    
    echo "  Task ID: $task_id"
    
    # Poll for completion
    local status="pending"
    local elapsed=0
    
    while [ "$status" != "completed" ] && [ "$status" != "failed" ] && [ $elapsed -lt $timeout_seconds ]; do
        sleep 3
        elapsed=$((elapsed + 3))
        
        local status_response
        if [ -n "$AUTH_HEADER" ]; then
            status_response=$(curl -s "$API_URL/api/task/$task_id" -H "$AUTH_HEADER" 2>&1)
        else
            status_response=$(curl -s "$API_URL/api/task/$task_id" 2>&1)
        fi
        
        status=$(echo "$status_response" | jq -r '.status // "unknown"' 2>/dev/null)
        local iterations=$(echo "$status_response" | jq -r '.iterations // 0' 2>/dev/null)
        echo -ne "\r  Status: $status (iter: $iterations, ${elapsed}s)    "
    done
    echo ""
    
    local end_time=$(date +%s)
    local duration=$((end_time - start_time))
    
    # Get final result
    local final_response
    if [ -n "$AUTH_HEADER" ]; then
        final_response=$(curl -s "$API_URL/api/task/$task_id" -H "$AUTH_HEADER")
    else
        final_response=$(curl -s "$API_URL/api/task/$task_id")
    fi
    
    local final_status=$(echo "$final_response" | jq -r '.status // "unknown"')
    local result=$(echo "$final_response" | jq -r '.result // ""')
    local result_length=${#result}
    
    # Save full result
    echo "$final_response" | jq . > "$RESULTS_DIR/${safe_name}.json" 2>/dev/null || echo "$final_response" > "$RESULTS_DIR/${safe_name}.json"
    
    # Determine quality score (simple heuristic)
    local quality="unknown"
    if [ "$final_status" = "completed" ]; then
        if [ $result_length -gt 500 ]; then
            quality="good"
        elif [ $result_length -gt 100 ]; then
            quality="partial"
        else
            quality="minimal"
        fi
    elif [ "$final_status" = "failed" ]; then
        quality="failed"
    else
        quality="timeout"
    fi
    
    echo "  Result: $final_status in ${duration}s, ${result_length} chars ($quality)"
    RESULTS+=("$model|$final_status|$duration|$result_length|$quality")
    
    return 0
}

# Run tests
echo ""
echo "Starting tests..."
echo ""

for model in "${MODELS[@]}"; do
    test_model "$model"
    sleep 1
done

# Print summary
echo ""
echo "==========================================="
echo "SUMMARY"
echo "==========================================="
echo ""
printf "%-45s | %-10s | %8s | %8s | %s\n" "Model" "Status" "Time(s)" "Chars" "Quality"
echo "---------------------------------------------+------------+----------+----------+----------"

for result in "${RESULTS[@]}"; do
    IFS='|' read -r model status duration chars quality <<< "$result"
    printf "%-45s | %-10s | %8s | %8s | %s\n" "$model" "$status" "$duration" "$chars" "$quality"
done

echo ""
echo "Full results saved to: $RESULTS_DIR"

# Save summary as JSON
{
    echo "["
    first=true
    for result in "${RESULTS[@]}"; do
        IFS='|' read -r model status duration chars quality <<< "$result"
        if [ "$first" = true ]; then
            first=false
        else
            echo ","
        fi
        echo "  {\"model\": \"$model\", \"status\": \"$status\", \"duration_seconds\": $duration, \"result_chars\": $chars, \"quality\": \"$quality\"}"
    done
    echo ""
    echo "]"
} > "$RESULTS_DIR/summary.json"

echo "Summary JSON: $RESULTS_DIR/summary.json"
