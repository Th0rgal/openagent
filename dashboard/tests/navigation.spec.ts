import { test, expect } from '@playwright/test';

test.describe('Navigation', () => {
  test('should navigate to all main pages', async ({ page }) => {
    await page.goto('/');

    // Check Overview page loads
    await expect(page.getByRole('heading', { name: /Overview|Dashboard/i })).toBeVisible();

    // Navigate to Mission
    await page.getByRole('link', { name: /Mission/i }).click();
    await expect(page).toHaveURL(/\/control/);

    // Navigate to Agents
    await page.getByRole('link', { name: /Agents/i }).click();
    await expect(page).toHaveURL(/\/agents/);
    await expect(page.getByRole('heading', { name: 'Agents' })).toBeVisible();

    // Navigate to Workspaces
    await page.getByRole('link', { name: /Workspaces/i }).click();
    await expect(page).toHaveURL(/\/workspaces/);
    await expect(page.getByRole('heading', { name: 'Workspaces' })).toBeVisible();

    // Navigate to Console
    await page.getByRole('link', { name: /Console/i }).click();
    await expect(page).toHaveURL(/\/console/);

    // Navigate to Settings
    await page.getByRole('link', { name: /Settings/i }).click();
    await expect(page).toHaveURL(/\/settings/);
  });

  test('should expand Library submenu', async ({ page }) => {
    await page.goto('/');

    // Click Library to expand
    await page.getByRole('link', { name: /Library/i }).click();

    // Should navigate to library page
    await expect(page).toHaveURL(/\/library/);
  });

  test('sidebar should be visible on all pages', async ({ page }) => {
    const pages = ['/', '/agents', '/workspaces', '/control', '/settings'];

    for (const pagePath of pages) {
      await page.goto(pagePath);

      // Sidebar should contain navigation links
      await expect(page.getByRole('link', { name: /Overview/i })).toBeVisible();
      await expect(page.getByRole('link', { name: /Mission/i })).toBeVisible();
      await expect(page.getByRole('link', { name: /Agents/i })).toBeVisible();
    }
  });
});
