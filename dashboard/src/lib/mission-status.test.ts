import { describe, it, expect } from 'vitest';
import {
  isFinishedStatus,
  needsAttentionStatus,
  categorizeMission,
  categorizeMissions,
  getMissionDotColor,
  getMissionTextColor,
  FINISHED_STATUSES,
  NEEDS_ATTENTION_STATUSES,
} from './mission-status';
import type { MissionStatus } from './api/missions';

describe('mission-status', () => {
  describe('isFinishedStatus', () => {
    it('returns true for finished statuses', () => {
      expect(isFinishedStatus('completed')).toBe(true);
      expect(isFinishedStatus('failed')).toBe(true);
      expect(isFinishedStatus('not_feasible')).toBe(true);
    });

    it('returns false for non-finished statuses', () => {
      expect(isFinishedStatus('active')).toBe(false);
      expect(isFinishedStatus('interrupted')).toBe(false);
      expect(isFinishedStatus('blocked')).toBe(false);
    });
  });

  describe('needsAttentionStatus', () => {
    it('returns true for attention-needed statuses', () => {
      expect(needsAttentionStatus('interrupted')).toBe(true);
      expect(needsAttentionStatus('blocked')).toBe(true);
    });

    it('returns false for other statuses', () => {
      expect(needsAttentionStatus('active')).toBe(false);
      expect(needsAttentionStatus('completed')).toBe(false);
      expect(needsAttentionStatus('failed')).toBe(false);
      expect(needsAttentionStatus('not_feasible')).toBe(false);
    });
  });

  describe('categorizeMission', () => {
    describe('running takes priority', () => {
      it('categorizes as running when actually running, regardless of stored status', () => {
        // Key scenario: resumed mission has "interrupted" status but is actually running
        expect(categorizeMission('interrupted', true)).toBe('running');
        expect(categorizeMission('active', true)).toBe('running');
        expect(categorizeMission('blocked', true)).toBe('running');
        // Even weird edge cases
        expect(categorizeMission('completed', true)).toBe('running');
        expect(categorizeMission('failed', true)).toBe('running');
      });
    });

    describe('needs-you when not running', () => {
      it('categorizes interrupted missions as needs-you when not running', () => {
        expect(categorizeMission('interrupted', false)).toBe('needs-you');
      });

      it('categorizes blocked missions as needs-you when not running', () => {
        expect(categorizeMission('blocked', false)).toBe('needs-you');
      });
    });

    describe('finished when not running', () => {
      it('categorizes completed missions as finished when not running', () => {
        expect(categorizeMission('completed', false)).toBe('finished');
      });

      it('categorizes failed missions as finished when not running', () => {
        expect(categorizeMission('failed', false)).toBe('finished');
      });

      it('categorizes not_feasible missions as finished when not running', () => {
        expect(categorizeMission('not_feasible', false)).toBe('finished');
      });
    });

    describe('other category', () => {
      it('categorizes active but not-running missions as other', () => {
        // Edge case: stored as active but runtime says not running
        // This can happen briefly during state transitions
        expect(categorizeMission('active', false)).toBe('other');
      });
    });
  });

  describe('categorizeMissions', () => {
    type TestMission = { id: string; status: MissionStatus };

    it('groups missions into correct categories', () => {
      const missions: TestMission[] = [
        { id: '1', status: 'active' },
        { id: '2', status: 'interrupted' },
        { id: '3', status: 'completed' },
        { id: '4', status: 'failed' },
        { id: '5', status: 'blocked' },
      ];
      const runningIds = new Set(['1']);

      const result = categorizeMissions(missions, runningIds);

      expect(result.running.map(m => m.id)).toEqual(['1']);
      expect(result['needs-you'].map(m => m.id)).toEqual(['2', '5']);
      expect(result.finished.map(m => m.id)).toEqual(['3', '4']);
      expect(result.other).toEqual([]);
    });

    it('handles resumed mission correctly (interrupted but running)', () => {
      const missions: TestMission[] = [
        { id: 'resumed', status: 'interrupted' }, // DB still says interrupted
        { id: 'new', status: 'active' },
        { id: 'waiting', status: 'interrupted' },
      ];
      // Both "resumed" and "new" are actually running
      const runningIds = new Set(['resumed', 'new']);

      const result = categorizeMissions(missions, runningIds);

      // Resumed mission should be in "running", not "needs-you"
      expect(result.running.map(m => m.id)).toEqual(['resumed', 'new']);
      expect(result['needs-you'].map(m => m.id)).toEqual(['waiting']);
      expect(result.finished).toEqual([]);
    });

    it('handles empty missions array', () => {
      const result = categorizeMissions([], new Set());

      expect(result.running).toEqual([]);
      expect(result['needs-you']).toEqual([]);
      expect(result.finished).toEqual([]);
      expect(result.other).toEqual([]);
    });

    it('handles empty running set', () => {
      const missions: TestMission[] = [
        { id: '1', status: 'active' },
        { id: '2', status: 'completed' },
      ];

      const result = categorizeMissions(missions, new Set());

      expect(result.running).toEqual([]);
      expect(result.other.map(m => m.id)).toEqual(['1']); // active but not running
      expect(result.finished.map(m => m.id)).toEqual(['2']);
    });

    it('puts each mission in exactly one category', () => {
      const missions: TestMission[] = [
        { id: '1', status: 'active' },
        { id: '2', status: 'interrupted' },
        { id: '3', status: 'completed' },
        { id: '4', status: 'blocked' },
        { id: '5', status: 'failed' },
        { id: '6', status: 'not_feasible' },
      ];
      const runningIds = new Set(['1', '2']); // Both active and interrupted are running

      const result = categorizeMissions(missions, runningIds);

      const allCategorized = [
        ...result.running,
        ...result['needs-you'],
        ...result.finished,
        ...result.other,
      ];

      // Total count should match
      expect(allCategorized.length).toBe(missions.length);

      // No duplicates
      const ids = allCategorized.map(m => m.id);
      expect(new Set(ids).size).toBe(ids.length);
    });
  });

  describe('getMissionDotColor', () => {
    it('returns indigo for running missions regardless of status', () => {
      expect(getMissionDotColor('active', true)).toBe('bg-indigo-400');
      expect(getMissionDotColor('interrupted', true)).toBe('bg-indigo-400');
      expect(getMissionDotColor('completed', true)).toBe('bg-indigo-400');
    });

    it('returns status-specific color when not running', () => {
      expect(getMissionDotColor('completed', false)).toBe('bg-emerald-400');
      expect(getMissionDotColor('failed', false)).toBe('bg-red-400');
      expect(getMissionDotColor('interrupted', false)).toBe('bg-amber-400');
      expect(getMissionDotColor('blocked', false)).toBe('bg-orange-400');
      expect(getMissionDotColor('not_feasible', false)).toBe('bg-rose-400');
      expect(getMissionDotColor('active', false)).toBe('bg-indigo-400');
    });
  });

  describe('getMissionTextColor', () => {
    it('returns indigo for running missions regardless of status', () => {
      expect(getMissionTextColor('active', true)).toBe('text-indigo-400');
      expect(getMissionTextColor('interrupted', true)).toBe('text-indigo-400');
    });

    it('returns status-specific color when not running', () => {
      expect(getMissionTextColor('completed', false)).toBe('text-emerald-400');
      expect(getMissionTextColor('failed', false)).toBe('text-red-400');
      expect(getMissionTextColor('interrupted', false)).toBe('text-amber-400');
    });
  });

  describe('status constants', () => {
    it('FINISHED_STATUSES contains expected values', () => {
      expect(FINISHED_STATUSES).toContain('completed');
      expect(FINISHED_STATUSES).toContain('failed');
      expect(FINISHED_STATUSES).toContain('not_feasible');
      expect(FINISHED_STATUSES).toHaveLength(3);
    });

    it('NEEDS_ATTENTION_STATUSES contains expected values', () => {
      expect(NEEDS_ATTENTION_STATUSES).toContain('interrupted');
      expect(NEEDS_ATTENTION_STATUSES).toContain('blocked');
      expect(NEEDS_ATTENTION_STATUSES).toHaveLength(2);
    });
  });
});
