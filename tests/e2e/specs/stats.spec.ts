import { test, expect } from '@playwright/test';
import { setupAppMock } from '../lib/mocks';

test.describe('Stats', () => {
  test.beforeEach(async ({ page }) => {
    await setupAppMock(page);
    
    await page.route('/api/models', async route => {
        await route.fulfill({ json: { provider: 'test', active_model: 'gpt-4', models: [] } });
    });
    await page.route('/api/conversations', async route => {
        await route.fulfill({ json: [] });
    });
    
    // Mock stats APIs
    await page.route('/api/stats/overview', async route => {
        await route.fulfill({ json: { total_conversations: 10, total_messages: 50, total_tokens: 1000, total_requests: 40 } });
    });
    await page.route('/api/stats/usage*', async route => {
        await route.fulfill({ json: { period: 'month', data: [] } });
    });
    await page.route('/api/stats/models*', async route => {
        await route.fulfill({ json: { period: 'month', data: [] } });
    });
    await page.route('/api/stats/conversations*', async route => {
        await route.fulfill({ json: { period: 'month', data: [] } });
    });
    await page.route('/api/stats/conversations-by-provider*', async route => {
        await route.fulfill({ json: { period: 'month', data: [] } });
    });
    await page.route('/api/stats/conversations-by-subagent*', async route => {
        await route.fulfill({ json: { period: 'month', data: [] } });
    });
  });

  test('should load stats dashboard', async ({ page }) => {
    await page.goto('/?tab=stats');
    
    // Check summary cards
    await expect(page.locator('#stat-conversations')).toHaveText('10');
    await expect(page.locator('#stat-messages')).toHaveText('50');
    
    // Check charts existence
    await expect(page.locator('#chart-tokens')).toBeVisible();
    await expect(page.locator('#chart-conversations')).toBeVisible();
  });
});
