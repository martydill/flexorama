import { test, expect } from '@playwright/test';
import { setupAppMock } from '../lib/mocks';

test.describe('Agents', () => {
  test.beforeEach(async ({ page }) => {
    await setupAppMock(page);

    await page.route('/api/models', async route => {
        await route.fulfill({ json: { provider: 'test', active_model: 'gpt-4', models: [] } });
    });
    await page.route('/api/conversations', async route => {
        await route.fulfill({ json: [] });
    });
     await page.route('/api/agents/active', async route => {
        await route.fulfill({ json: { active: null } });
    });
  });

  test('should list agents', async ({ page }) => {
    const mockAgents = [
      { name: 'Developer', model: 'gpt-4', allowed_tools: ['read_file'], denied_tools: [], system_prompt: 'You are a dev.' },
      { name: 'Writer', model: 'claude-3', allowed_tools: [], denied_tools: [], system_prompt: 'You are a writer.' }
    ];

    await page.route('/api/agents', async route => {
      await route.fulfill({ json: mockAgents });
    });

    await page.goto('/?tab=agents');
    
    await expect(page.locator('#agent-list .list-item')).toHaveCount(2);
    await expect(page.locator('#agent-list .list-item').first()).toContainText('Developer');
  });

  test('should create agent', async ({ page }) => {
     await page.route('/api/agents', async route => {
        if (route.request().method() === 'GET') await route.fulfill({ json: [] });
        else await route.fulfill({ json: { name: 'NewAgent' } });
    });

    await page.goto('/?tab=agents');
    await page.click('#new-agent');
    
    await page.fill('#agent-name', 'NewAgent');
    await page.fill('#agent-prompt', 'System prompt');
    
    await page.click('#save-agent');
  });

  test('should update agent', async ({ page }) => {
    const agent = { name: 'Dev', model: 'gpt-4', allowed_tools: [], denied_tools: [], system_prompt: 'Old prompt' };
    await page.route('/api/agents', async route => {
        await route.fulfill({ json: [agent] });
    });

    let updateCalled = false;
    await page.route('/api/agents/Dev', async route => {
        if (route.request().method() === 'PUT') {
            updateCalled = true;
            const body = route.request().postDataJSON();
            expect(body.system_prompt).toBe('New prompt');
            await route.fulfill({ json: { ...agent, ...body } });
        } else {
            await route.continue();
        }
    });

    await page.goto('/?tab=agents');
    await page.click('#agent-list .list-item');
    await page.fill('#agent-prompt', 'New prompt');
    await page.click('#save-agent');
    
    expect(updateCalled).toBe(true);
  });

  test('should delete agent', async ({ page }) => {
    const agent = { name: 'Dev', model: 'gpt-4', allowed_tools: [], denied_tools: [], system_prompt: 'Old prompt' };
    await page.route('/api/agents', async route => {
        await route.fulfill({ json: [agent] });
    });

    let deleteCalled = false;
    await page.route('/api/agents/Dev', async route => {
        if (route.request().method() === 'DELETE') {
            deleteCalled = true;
            await route.fulfill({ json: { success: true } });
        }
    });

    await page.goto('/?tab=agents');
    await page.click('#agent-list .list-item');
    
    await expect(page.locator('#delete-agent')).toBeVisible();
    await page.click('#delete-agent');
    
    expect(deleteCalled).toBe(true);
  });
});
