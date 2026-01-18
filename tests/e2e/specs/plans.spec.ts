import { test, expect } from '@playwright/test';
import { setupAppMock } from '../lib/mocks';

test.describe('Plans', () => {
  test.beforeEach(async ({ page }) => {
    await setupAppMock(page);

    // Mock common endpoints to avoid errors
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
    await page.route('/api/mcp/servers', async route => {
      await route.fulfill({ json: [] });
    });
    await page.route('/api/agents', async route => {
      await route.fulfill({ json: [] });
    });
    await page.route('/api/skills', async route => {
      await route.fulfill({ json: [] });
    });
    await page.route('/api/commands', async route => {
      await route.fulfill({ json: [] });
    });
  });

  test('should list plans', async ({ page }) => {
    const mockPlans = [
      { id: '1', title: 'Plan A', user_request: 'Do A', plan_markdown: '# Plan A', created_at: new Date().toISOString() },
      { id: '2', title: 'Plan B', user_request: 'Do B', plan_markdown: '# Plan B', created_at: new Date().toISOString() }
    ];

    await page.route('/api/plans', async route => {
      await route.fulfill({ json: mockPlans });
    });

    await page.goto('/?tab=plans');
    
    await expect(page.locator('#plan-list .list-item')).toHaveCount(2);
    await expect(page.locator('#plan-list .list-item').first()).toContainText('Plan A');
  });

  test('should create plan', async ({ page }) => {
    await page.route('/api/plans', async route => {
      if (route.request().method() === 'GET') {
        await route.fulfill({ json: [] });
      } else if (route.request().method() === 'POST') {
        await route.fulfill({ json: { id: 'new-plan' } });
      }
    });

    await page.goto('/?tab=plans');
    await page.click('#create-plan-sidebar');
    
    // Should clear inputs
    await expect(page.locator('#plan-title')).toHaveValue('');
    await expect(page.locator('#plan-user-request')).toHaveValue('');
  });

  test('should select and save plan', async ({ page }) => {
    const mockPlan = { id: '1', title: 'Plan A', user_request: 'Do A', plan_markdown: '# Plan A', created_at: new Date().toISOString() };
    
    await page.route('/api/plans', async route => {
      await route.fulfill({ json: [mockPlan] });
    });

    await page.route('/api/plans/1', async route => {
        if (route.request().method() === 'PUT') {
            const data = route.request().postDataJSON();
            await route.fulfill({ json: { ...mockPlan, ...data } });
        } else {
             await route.fulfill({ json: mockPlan });
        }
    });

    await page.goto('/?tab=plans');
    await page.click('#plan-list .list-item');
    
    // Check values loaded
    await expect(page.locator('#plan-title')).toHaveValue('Plan A');
    
    // Edit and save
    await page.fill('#plan-title', 'Plan A Modified');
    await page.click('#save-plan');
    
    // Verify PUT request happened (implicitly by test passing if we asserted on it, but here we mock the response)
    // To strictly verify, we could track requests
  });

  test('should show edit plan button in chat and navigate to plan', async ({ page }) => {
    const planId = 'test-plan-123';
    const mockPlan = { 
      id: planId, 
      title: 'Generated Plan', 
      user_request: 'Make a plan', 
      plan_markdown: '# The Plan', 
      created_at: new Date().toISOString() 
    };

    // Mock plans
    await page.route('/api/plans', async route => {
      await route.fulfill({ json: [mockPlan] });
    });

    // Mock conversation with plan message
    await page.route('/api/conversations/123', async route => {
      await route.fulfill({ json: {
        conversation: { id: '123', updated_at: new Date().toISOString() },
        messages: [
          { role: 'user', content: 'Create a plan', blocks: [] },
          { role: 'assistant', content: `_Plan saved with ID: \`${planId}\`._`, blocks: [] }
        ],
        context_files: []
      }});
    });

    // Mock conversation list
    await page.route('/api/conversations', async route => {
      if (route.request().method() === 'GET') {
        await route.fulfill({ json: [{ id: '123', last_message: 'Plan saved', updated_at: new Date().toISOString() }] });
      } else {
        await route.fulfill({ status: 404, body: 'Not found' });
      }
    });
    
    // Mock pending permissions
    await page.route('/api/permissions/pending?conversation_id=123', async route => {
      await route.fulfill({ json: [] });
    });

    await page.goto('/?tab=chats');
    await page.click('#conversation-list .list-item'); // Select the conversation

    // Check for button
    const editBtn = page.getByRole('button', { name: 'Edit Plan' });
    await expect(editBtn).toBeVisible();

    // Click button
    await editBtn.click();

    // Verify tab switch
    await expect(page.locator('.top-tab[data-tab="plans"]')).toHaveClass(/active/);
    await expect(page.locator('#tab-plans')).toHaveClass(/active/);

    // Verify plan loaded
    await expect(page.locator('#plan-title')).toHaveValue('Generated Plan');
    await expect(page.locator('#plan-markdown')).toHaveValue('# The Plan');
  });
});
