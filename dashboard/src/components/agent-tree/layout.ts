/**
 * Tree Layout Algorithm
 * 
 * Computes positions for nodes in a tree structure using a modified
 * Reingold-Tilford algorithm for aesthetic tree layouts.
 */

import type { AgentNode, LayoutNode, LayoutEdge, TreeLayout } from './types';

const NODE_WIDTH = 140;
const NODE_HEIGHT = 80;
const HORIZONTAL_GAP = 60;
const VERTICAL_GAP = 120;

interface NodePosition {
  id: string;
  x: number;
  y: number;
  agent: AgentNode;
}

/**
 * Compute tree layout with proper spacing and centering
 */
export function computeLayout(root: AgentNode | null): TreeLayout {
  if (!root) {
    return { nodes: [], edges: [], width: 0, height: 0 };
  }

  // Map to store computed positions
  const positions = new Map<string, NodePosition>();
  let nextLeafX = 0;

  // First pass: compute positions recursively (post-order traversal)
  function computePositions(node: AgentNode, depth: number): number {
    const y = depth * (NODE_HEIGHT + VERTICAL_GAP) + 50;
    
    if (!node.children || node.children.length === 0) {
      // Leaf node - assign next available x
      const x = nextLeafX;
      nextLeafX += NODE_WIDTH + HORIZONTAL_GAP;
      
      positions.set(node.id, { id: node.id, x, y, agent: node });
      return x + NODE_WIDTH / 2; // Return center x
    }
    
    // Internal node - first position all children
    const childCenters: number[] = [];
    for (const child of node.children) {
      const childCenter = computePositions(child, depth + 1);
      childCenters.push(childCenter);
    }
    
    // Center this node over its children
    const leftmostCenter = Math.min(...childCenters);
    const rightmostCenter = Math.max(...childCenters);
    const centerX = (leftmostCenter + rightmostCenter) / 2;
    const x = centerX - NODE_WIDTH / 2;
    
    positions.set(node.id, { id: node.id, x, y, agent: node });
    return centerX;
  }

  computePositions(root, 0);

  // Second pass: create edges by walking the tree
  const edges: LayoutEdge[] = [];
  
  function createEdges(node: AgentNode) {
    const parentPos = positions.get(node.id);
    if (!parentPos) return;
    
    if (node.children) {
      for (const child of node.children) {
        const childPos = positions.get(child.id);
        if (childPos) {
          edges.push({
            id: `edge-${node.id}-${child.id}`,
            from: {
              x: parentPos.x + NODE_WIDTH / 2,
              y: parentPos.y + NODE_HEIGHT,
            },
            to: {
              x: childPos.x + NODE_WIDTH / 2,
              y: childPos.y,
            },
            status: child.status,
          });
        }
        createEdges(child);
      }
    }
  }

  createEdges(root);

  // Convert positions map to array
  const nodes: LayoutNode[] = Array.from(positions.values());

  // Compute bounds
  let minX = Infinity, maxX = -Infinity, maxY = 0;
  for (const node of nodes) {
    minX = Math.min(minX, node.x);
    maxX = Math.max(maxX, node.x + NODE_WIDTH);
    maxY = Math.max(maxY, node.y + NODE_HEIGHT);
  }

  // Normalize positions (shift to start at 0 with padding)
  const offsetX = -minX + 50;
  for (const node of nodes) {
    node.x += offsetX;
  }
  for (const edge of edges) {
    edge.from.x += offsetX;
    edge.to.x += offsetX;
  }

  return {
    nodes,
    edges,
    width: maxX - minX + 100,
    height: maxY + 100,
  };
}

/**
 * Get count statistics for a tree
 */
export function getTreeStats(root: AgentNode | null): { 
  total: number; 
  running: number; 
  completed: number; 
  failed: number;
  pending: number;
} {
  if (!root) return { total: 0, running: 0, completed: 0, failed: 0, pending: 0 };
  
  let stats = {
    total: 1,
    running: root.status === 'running' ? 1 : 0,
    completed: root.status === 'completed' ? 1 : 0,
    failed: root.status === 'failed' ? 1 : 0,
    pending: root.status === 'pending' ? 1 : 0,
  };
  
  if (root.children) {
    for (const child of root.children) {
      const childStats = getTreeStats(child);
      stats.total += childStats.total;
      stats.running += childStats.running;
      stats.completed += childStats.completed;
      stats.failed += childStats.failed;
      stats.pending += childStats.pending;
    }
  }
  
  return stats;
}

/**
 * Get all node IDs in a tree
 */
export function getAllNodeIds(root: AgentNode | null): Set<string> {
  const ids = new Set<string>();
  
  function walk(node: AgentNode) {
    ids.add(node.id);
    if (node.children) {
      for (const child of node.children) {
        walk(child);
      }
    }
  }
  
  if (root) walk(root);
  return ids;
}
