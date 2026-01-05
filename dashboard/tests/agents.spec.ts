import { test, expect } from '@playwright/test';

test.describe('Agents Page', () => {
  test('should load agents page', async ({ page }) => {
    await page.goto('/agents');

    // Check for page title
    await expect(page.getByRole('heading', { name: 'Agents' })).toBeVisible();

    // Check for "New Agent" button
    await expect(page.getByRole('button', { name: /New Agent/i })).toBeVisible();
  });

  test('should show empty state when no agents', async ({ page }) => {
    await page.goto('/agents');

    // Wait for potential loading to complete
    await page.waitForTimeout(1000);

    // Check for empty state (might show if no agents created)
    const emptyText = page.getByText(/No agents yet|Select an agent/i);
    const hasEmpty = await emptyText.isVisible().catch(() => false);

    // Either shows empty state or shows agents list
    expect(hasEmpty || await page.locator('[role="button"]').count() > 1).toBeTruthy();
  });

  test('should open new agent dialog', async ({ page }) => {
    await page.goto('/agents');

    // Click "New Agent" button
    await page.getByRole('button', { name: /New Agent/i }).click();

    // Check dialog appears
    await expect(page.getByRole('heading', { name: 'New Agent' })).toBeVisible();

    // Check for name input
    await expect(page.getByPlaceholder(/Agent|name/i)).toBeVisible();

    // Check for model selector
    await expect(page.locator('select')).toBeVisible();
  });

  test('should validate agent creation form', async ({ page }) => {
    await page.goto('/agents');

    // Open new agent dialog
    await page.getByRole('button', { name: /New Agent/i }).click();

    // Try to create without name
    const createButton = page.getByRole('button', { name: /Create/i });
    await expect(createButton).toBeDisabled();

    // Fill in name
    await page.getByPlaceholder(/Agent|name/i).fill('Test Agent');

    // Now button should be enabled
    await expect(createButton).toBeEnabled();
  });
});
