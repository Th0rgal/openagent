'use client';

import { useEffect, useMemo, useState } from 'react';
import { cn } from '@/lib/utils';
import { listTasks, TaskState } from '@/lib/api';
import { formatCents } from '@/lib/utils';
import {
  Bot,
  Brain,
  Cpu,
  CheckCircle,
  XCircle,
  Loader,
  Clock,
  Ban,
  ChevronRight,
  ChevronDown,
  Zap,
  GitBranch,
  Target,
} from 'lucide-react';

// Mock agent tree structure (in production, this would come from the API)
interface AgentNode {
  id: string;
  type: 'Root' | 'Node' | 'ComplexityEstimator' | 'ModelSelector' | 'TaskExecutor' | 'Verifier';
  status: 'running' | 'completed' | 'failed' | 'pending' | 'paused' | 'cancelled';
  name: string;
  description: string;
  budgetAllocated: number;
  budgetSpent: number;
  children?: AgentNode[];
  logs?: string[];
  selectedModel?: string;
  complexity?: number;
}

const agentIcons = {
  Root: Bot,
  Node: GitBranch,
  ComplexityEstimator: Brain,
  ModelSelector: Cpu,
  TaskExecutor: Zap,
  Verifier: Target,
};

const statusColors = {
  running: 'border-[var(--accent)] bg-[var(--accent)]/10',
  completed: 'border-[var(--success)] bg-[var(--success)]/10',
  failed: 'border-[var(--error)] bg-[var(--error)]/10',
  pending: 'border-[var(--warning)] bg-[var(--warning)]/10',
  paused: 'border-[var(--foreground-muted)] bg-[var(--foreground-muted)]/10',
  cancelled: 'border-[var(--foreground-muted)] bg-[var(--foreground-muted)]/10',
};

const statusTextColors = {
  running: 'text-[var(--accent)]',
  completed: 'text-[var(--success)]',
  failed: 'text-[var(--error)]',
  pending: 'text-[var(--warning)]',
  paused: 'text-[var(--foreground-muted)]',
  cancelled: 'text-[var(--foreground-muted)]',
};

function mapTaskStatusToAgentStatus(status: TaskState['status']): AgentNode['status'] {
  switch (status) {
    case 'running':
      return 'running';
    case 'completed':
      return 'completed';
    case 'failed':
      return 'failed';
    case 'pending':
      return 'pending';
    case 'cancelled':
      return 'cancelled';
  }
}

function AgentTreeNode({
  agent,
  depth = 0,
  onSelect,
  selectedId,
}: {
  agent: AgentNode;
  depth?: number;
  onSelect: (agent: AgentNode) => void;
  selectedId: string | null;
}) {
  const [expanded, setExpanded] = useState(true);
  const Icon = agentIcons[agent.type];
  const hasChildren = agent.children && agent.children.length > 0;
  const isSelected = selectedId === agent.id;

  return (
    <div style={{ marginLeft: depth * 24 }}>
      <div
        className={cn(
          'group flex cursor-pointer items-center gap-2 rounded-lg border p-3 transition-all',
          statusColors[agent.status],
          isSelected && 'ring-2 ring-[var(--accent)]'
        )}
        onClick={() => onSelect(agent)}
      >
        {hasChildren && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              setExpanded(!expanded);
            }}
            className="p-1"
          >
            {expanded ? (
              <ChevronDown className="h-4 w-4 text-[var(--foreground-muted)]" />
            ) : (
              <ChevronRight className="h-4 w-4 text-[var(--foreground-muted)]" />
            )}
          </button>
        )}

        <div
          className={cn(
            'flex h-8 w-8 items-center justify-center rounded-lg',
            statusColors[agent.status]
          )}
        >
          <Icon className={cn('h-4 w-4', statusTextColors[agent.status])} />
        </div>

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="font-medium text-[var(--foreground)]">{agent.name}</span>
            <span className="text-xs text-[var(--foreground-muted)]">{agent.type}</span>
          </div>
          <p className="truncate text-xs text-[var(--foreground-muted)]">{agent.description}</p>
        </div>

        <div className="flex items-center gap-3">
          {/* Status indicator */}
          {agent.status === 'running' && (
            <Loader className={cn('h-4 w-4 animate-spin', statusTextColors[agent.status])} />
          )}
          {agent.status === 'completed' && (
            <CheckCircle className={cn('h-4 w-4', statusTextColors[agent.status])} />
          )}
          {agent.status === 'failed' && (
            <XCircle className={cn('h-4 w-4', statusTextColors[agent.status])} />
          )}
          {agent.status === 'pending' && (
            <Clock className={cn('h-4 w-4', statusTextColors[agent.status])} />
          )}
          {agent.status === 'cancelled' && (
            <Ban className={cn('h-4 w-4', statusTextColors[agent.status])} />
          )}

          {/* Budget */}
          <div className="text-right">
            <div className="text-xs text-[var(--foreground-muted)]">Budget</div>
            <div className="text-sm font-medium text-[var(--foreground)]">
              {formatCents(agent.budgetSpent)} / {formatCents(agent.budgetAllocated)}
            </div>
          </div>
        </div>
      </div>

      {/* Children */}
      {hasChildren && expanded && (
        <div className="mt-2 space-y-2">
          {agent.children!.map((child) => (
            <AgentTreeNode
              key={child.id}
              agent={child}
              depth={depth + 1}
              onSelect={onSelect}
              selectedId={selectedId}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export default function AgentsPage() {
  const [tasks, setTasks] = useState<TaskState[]>([]);
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const selectedTask = useMemo(
    () => tasks.find((t) => t.id === selectedTaskId) ?? null,
    [tasks, selectedTaskId]
  );
  const [selectedAgent, setSelectedAgent] = useState<AgentNode | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    let seq = 0;

    const fetchTasks = async () => {
      const mySeq = ++seq;
      try {
        const data = await listTasks();
        if (cancelled || mySeq !== seq) return;

        setTasks(data);
        setSelectedTaskId((prev) => {
          if (data.length === 0) return null;
          if (!prev) return data[0]!.id;
          const stillExists = data.some((t) => t.id === prev);
          return stillExists ? prev : data[0]!.id;
        });
      } catch (error) {
        console.error('Failed to fetch tasks:', error);
      } finally {
        if (!cancelled && mySeq === seq) {
          setLoading(false);
        }
      }
    };

    fetchTasks();
    const interval = setInterval(fetchTasks, 3000);
    return () => {
      cancelled = true;
      seq += 1; // invalidate any in-flight request
      clearInterval(interval);
    };
  }, []);

  // Mock agent tree for the selected task
  const mockAgentTree: AgentNode | null = selectedTask
    ? {
        id: 'root',
        type: 'Root',
        status: mapTaskStatusToAgentStatus(selectedTask.status),
        name: 'Root Agent',
        description: selectedTask.task.slice(0, 50) + '...',
        budgetAllocated: 1000,
        budgetSpent: 50,
        children: [
          {
            id: 'complexity',
            type: 'ComplexityEstimator',
            status: 'completed',
            name: 'Complexity Estimator',
            description: 'Estimate task difficulty',
            budgetAllocated: 10,
            budgetSpent: 5,
            complexity: 0.6,
          },
          {
            id: 'model-selector',
            type: 'ModelSelector',
            status: 'completed',
            name: 'Model Selector',
            description: 'Select optimal model',
            budgetAllocated: 10,
            budgetSpent: 3,
            selectedModel: selectedTask.model,
          },
          {
            id: 'executor',
            type: 'TaskExecutor',
            status: mapTaskStatusToAgentStatus(selectedTask.status),
            name: 'Task Executor',
            description: 'Execute using tools',
            budgetAllocated: 900,
            budgetSpent: 35,
            logs: selectedTask.log.map((l) => l.content),
          },
          {
            id: 'verifier',
            type: 'Verifier',
            status:
              selectedTask.status === 'completed'
                ? 'completed'
                : selectedTask.status === 'failed'
                  ? 'failed'
                  : selectedTask.status === 'cancelled'
                    ? 'cancelled'
                    : 'pending',
            name: 'Verifier',
            description: 'Verify task completion',
            budgetAllocated: 80,
            budgetSpent: selectedTask.status === 'completed' ? 7 : 0,
          },
        ],
      }
    : null;

  return (
    <div className="flex h-screen">
      {/* Task selector sidebar */}
      <div className="w-64 border-r border-[var(--border)] bg-[var(--background-secondary)]/70 backdrop-blur p-4">
        <h2 className="mb-4 text-sm font-semibold text-[var(--foreground)]">Tasks</h2>
        <div className="space-y-2">
          {tasks.map((task) => (
            <button
              key={task.id}
              onClick={() => setSelectedTaskId(task.id)}
              className={cn(
                'w-full rounded-lg p-3 text-left transition-colors',
                selectedTaskId === task.id
                  ? 'bg-[var(--accent)]/10 border border-[var(--accent)]'
                  : 'bg-[var(--background-tertiary)] hover:bg-[var(--border)]'
              )}
            >
              <div className="flex items-center gap-2">
                {task.status === 'running' && (
                  <Loader className="h-3 w-3 animate-spin text-[var(--accent)]" />
                )}
                {task.status === 'completed' && (
                  <CheckCircle className="h-3 w-3 text-[var(--success)]" />
                )}
                {task.status === 'failed' && (
                  <XCircle className="h-3 w-3 text-[var(--error)]" />
                )}
                {task.status === 'pending' && (
                  <Clock className="h-3 w-3 text-[var(--warning)]" />
                )}
                {task.status === 'cancelled' && (
                  <Ban className="h-3 w-3 text-[var(--foreground-muted)]" />
                )}
                <span className="truncate text-sm text-[var(--foreground)]">
                  {task.task.slice(0, 30)}...
                </span>
              </div>
            </button>
          ))}
        </div>
      </div>

      {/* Agent tree */}
      <div className="flex-1 overflow-auto p-6">
        <div className="mb-6">
          <h1 className="text-2xl font-bold text-[var(--foreground)]">Agent Tree</h1>
          <p className="text-sm text-[var(--foreground-muted)]">
            Visualize the hierarchical agent structure
          </p>
        </div>

        {loading ? (
          <div className="flex items-center justify-center py-12">
            <Loader className="h-8 w-8 animate-spin text-[var(--accent)]" />
          </div>
        ) : mockAgentTree ? (
          <div className="space-y-2">
            <AgentTreeNode
              agent={mockAgentTree}
              onSelect={setSelectedAgent}
              selectedId={selectedAgent?.id || null}
            />
          </div>
        ) : (
          <div className="flex items-center justify-center py-12">
            <p className="text-[var(--foreground-muted)]">Select a task to view agent tree</p>
          </div>
        )}
      </div>

      {/* Agent details panel */}
      {selectedAgent && (
        <div className="w-80 border-l border-[var(--border)] bg-[var(--background-secondary)]/70 backdrop-blur p-4">
          <h2 className="mb-4 text-lg font-semibold text-[var(--foreground)]">
            {selectedAgent.name}
          </h2>

          <div className="space-y-4">
            {/* Type */}
            <div>
              <label className="text-xs text-[var(--foreground-muted)]">Type</label>
              <p className="text-sm text-[var(--foreground)]">{selectedAgent.type}</p>
            </div>

            {/* Status */}
            <div>
              <label className="text-xs text-[var(--foreground-muted)]">Status</label>
              <p className={cn('text-sm capitalize', statusTextColors[selectedAgent.status])}>
                {selectedAgent.status}
              </p>
            </div>

            {/* Description */}
            <div>
              <label className="text-xs text-[var(--foreground-muted)]">Description</label>
              <p className="text-sm text-[var(--foreground)]">{selectedAgent.description}</p>
            </div>

            {/* Budget */}
            <div>
              <label className="text-xs text-[var(--foreground-muted)]">Budget</label>
              <div className="mt-1">
                <div className="flex justify-between text-sm">
                  <span className="text-[var(--foreground)]">
                    {formatCents(selectedAgent.budgetSpent)}
                  </span>
                  <span className="text-[var(--foreground-muted)]">
                    of {formatCents(selectedAgent.budgetAllocated)}
                  </span>
                </div>
                <div className="mt-1 h-2 overflow-hidden rounded-sm bg-[var(--background-tertiary)]/70">
                  <div
                    className="h-full rounded-sm bg-[var(--accent)]"
                    style={{
                      width: `${Math.min(100, (selectedAgent.budgetSpent / selectedAgent.budgetAllocated) * 100)}%`,
                    }}
                  />
                </div>
              </div>
            </div>

            {/* Complexity (if applicable) */}
            {selectedAgent.complexity !== undefined && (
              <div>
                <label className="text-xs text-[var(--foreground-muted)]">Complexity Score</label>
                <p className="text-sm text-[var(--foreground)]">
                  {(selectedAgent.complexity * 100).toFixed(0)}%
                </p>
              </div>
            )}

            {/* Selected Model (if applicable) */}
            {selectedAgent.selectedModel && (
              <div>
                <label className="text-xs text-[var(--foreground-muted)]">Selected Model</label>
                <p className="text-sm text-[var(--foreground)]">{selectedAgent.selectedModel}</p>
              </div>
            )}

            {/* Logs */}
            {selectedAgent.logs && selectedAgent.logs.length > 0 && (
              <div>
                <label className="text-xs text-[var(--foreground-muted)]">
                  Logs ({selectedAgent.logs.length})
                </label>
                <div className="mt-2 max-h-64 space-y-2 overflow-auto">
                  {selectedAgent.logs.map((log, i) => (
                    <div
                      key={i}
                      className="rounded bg-[var(--background-tertiary)] p-2 text-xs font-mono text-[var(--foreground-muted)]"
                    >
                      {log.slice(0, 100)}...
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

