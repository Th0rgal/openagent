/**
 * Mission status utilities - shared logic for categorizing missions
 * based on runtime state and stored status.
 */

import type { MissionStatus } from './api/missions';

export type MissionCategory = 'running' | 'needs-you' | 'finished' | 'other';

export const FINISHED_STATUSES: MissionStatus[] = ['completed', 'failed', 'not_feasible'];
export const NEEDS_ATTENTION_STATUSES: MissionStatus[] = ['interrupted', 'blocked'];

/**
 * Check if a mission is in a finished state based on its stored status.
 */
export function isFinishedStatus(status: MissionStatus): boolean {
  return FINISHED_STATUSES.includes(status);
}

/**
 * Check if a mission needs user attention based on its stored status.
 */
export function needsAttentionStatus(status: MissionStatus): boolean {
  return NEEDS_ATTENTION_STATUSES.includes(status);
}

/**
 * Categorize a mission based on runtime state and stored status.
 * 
 * Priority order:
 * 1. Running - mission is actually running (runtime state takes precedence)
 * 2. Needs You - interrupted/blocked AND not running
 * 3. Finished - completed/failed/not_feasible AND not running
 * 4. Other - anything else (e.g., active but not in runtime running set)
 */
export function categorizeMission(
  status: MissionStatus,
  isActuallyRunning: boolean
): MissionCategory {
  if (isActuallyRunning) {
    return 'running';
  }
  
  if (needsAttentionStatus(status)) {
    return 'needs-you';
  }
  
  if (isFinishedStatus(status)) {
    return 'finished';
  }
  
  return 'other';
}

/**
 * Categorize multiple missions into columns for display.
 * Returns missions grouped by category with each mission only in one category.
 */
export function categorizeMissions<T extends { id: string; status: MissionStatus }>(
  missions: T[],
  runningMissionIds: Set<string>
): Record<MissionCategory, T[]> {
  const result: Record<MissionCategory, T[]> = {
    running: [],
    'needs-you': [],
    finished: [],
    other: [],
  };

  for (const mission of missions) {
    const isActuallyRunning = runningMissionIds.has(mission.id);
    const category = categorizeMission(mission.status, isActuallyRunning);
    result[category].push(mission);
  }

  return result;
}

/**
 * Status display utilities
 */
export const STATUS_DOT_COLORS: Record<MissionStatus, string> = {
  active: 'bg-indigo-400',
  completed: 'bg-emerald-400',
  failed: 'bg-red-400',
  interrupted: 'bg-amber-400',
  blocked: 'bg-orange-400',
  not_feasible: 'bg-rose-400',
};

export const STATUS_TEXT_COLORS: Record<MissionStatus, string> = {
  active: 'text-indigo-400',
  completed: 'text-emerald-400',
  failed: 'text-red-400',
  interrupted: 'text-amber-400',
  blocked: 'text-orange-400',
  not_feasible: 'text-rose-400',
};

export const STATUS_LABELS: Record<MissionStatus, string> = {
  active: 'Active',
  completed: 'Completed',
  failed: 'Failed',
  interrupted: 'Interrupted',
  blocked: 'Blocked',
  not_feasible: 'Not Feasible',
};

/**
 * Get the display dot color for a mission, considering runtime state.
 * Running missions always show indigo regardless of stored status.
 */
export function getMissionDotColor(status: MissionStatus, isActuallyRunning: boolean): string {
  if (isActuallyRunning) {
    return 'bg-indigo-400';
  }
  return STATUS_DOT_COLORS[status] || 'bg-gray-400';
}

/**
 * Get the display text color for a mission, considering runtime state.
 */
export function getMissionTextColor(status: MissionStatus, isActuallyRunning: boolean): string {
  if (isActuallyRunning) {
    return 'text-indigo-400';
  }
  return STATUS_TEXT_COLORS[status] || 'text-white/40';
}
