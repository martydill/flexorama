import { test, expect } from '@playwright/test';
import { setupAppMock } from '../lib/mocks';

test.describe('Commands', () => {
  test.beforeEach(async ({ page }) => {
    await setupAppMock(page);

    await page.route('/api/models', async route => {
        await route.fulfill({ json: { provider: 'test', active_model: 'gpt-4', models: [] } });
    });
    await page.route('/api/conversations', async route => {
        if (route.request().method() === 'GET') {
            await route.fulfill({ json: [] });
        } else {
            await route.fulfill({ status: 404, body: 'Not found' });
        }
    });
    await page.route('/api/plans', async route => {
        await route.fulfill({ json: [] });
    });
    await page.route('/api/mcp/servers', async route => {
        await route.fulfill({ json: [] });
    });
    await page.route('/api/agents', async route => {
        await route.fulfill({ json: [] });
    });
    await page.route('/api/skills', async route => {
        await route.fulfill({ json: [] });
    });
  });

  test('should list commands', async ({ page }) => {
    const mockCommands = [
      { name: 'test', description: 'Test command', allowed_tools: [], content: 'test' },
      { name: 'help', description: 'Help command', allowed_tools: [], content: 'help' }
    ];

    await page.route('/api/commands', async route => {
      await route.fulfill({ json: mockCommands });
    });

    await page.goto('/?tab=commands');
    
    await expect(page.locator('#command-list .list-item')).toHaveCount(2);
    await expect(page.locator('#command-list .list-item').first()).toContainText('/test');
  });

  test('should create command', async ({ page }) => {
    await page.route('/api/commands', async route => {
        if (route.request().method() === 'GET') await route.fulfill({ json: [] });
        else await route.fulfill({ json: { name: 'foo' } });
    });

    await page.goto('/?tab=commands');
    await page.click('#new-command');
    
    await page.fill('#command-name', 'foo');
    await page.fill('#command-description', 'Foo command');
    await page.fill('#command-content', 'Bar');
    
    await page.click('#save-command');
  });

  test('should update command', async ({ page }) => {
    const cmd = { name: 'foo', description: 'Old desc', content: 'Old content', allowed_tools: [] };
    await page.route('/api/commands', async route => {
        await route.fulfill({ json: [cmd] });
    });

    let updateCalled = false;
    await page.route('/api/commands/foo', async route => {
        if (route.request().method() === 'PUT') {
            updateCalled = true;
            const body = route.request().postDataJSON();
            expect(body.description).toBe('New desc');
            await route.fulfill({ json: { ...cmd, ...body } });
        } else {
            await route.continue();
        }
    });

    await page.goto('/?tab=commands');
    await page.click('#command-list .list-item');
    await page.fill('#command-description', 'New desc');
    await page.click('#save-command');
    
    expect(updateCalled).toBe(true);
  });

  test('should delete command', async ({ page }) => {
    const cmd = { name: 'foo', description: 'Desc', content: 'Content', allowed_tools: [] };
    await page.route('/api/commands', async route => {
        await route.fulfill({ json: [cmd] });
    });

    let deleteCalled = false;
    await page.route('/api/commands/foo', async route => {
        if (route.request().method() === 'DELETE') {
            deleteCalled = true;
            await route.fulfill({ json: { success: true } });
        }
    });

    await page.goto('/?tab=commands');
    await page.click('#command-list .list-item');
    
    await expect(page.locator('#delete-command')).toBeVisible();
    await page.click('#delete-command');
    
    expect(deleteCalled).toBe(true);
  });
});
