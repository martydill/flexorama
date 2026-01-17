import { test, expect } from '@playwright/test';
import { setupAppMock } from '../lib/mocks';

test.describe('Conversations', () => {
  test.beforeEach(async ({ page }) => {
    await setupAppMock(page);
    
    // Mock models
    await page.route('/api/models', async route => {
      await route.fulfill({ json: { provider: 'test', active_model: 'gpt-4', models: ['gpt-4', 'claude-3'] } });
    });

    // Mock agents
    await page.route('/api/agents', async route => {
      await route.fulfill({ json: [] });
    });
    await page.route('/api/agents/active', async route => {
      await route.fulfill({ json: { active: null } });
    });

    // Mock todos
    await page.route('/api/todos*', async route => {
      await route.fulfill({ json: [] });
    });
    
    // Mock permissions
    await page.route('/api/permissions/pending*', async route => {
      await route.fulfill({ json: [] });
    });
  });

  test('should load conversations list', async ({ page }) => {
    const mockConvs = [
      { id: '1', updated_at: new Date(Date.now() + 10000).toISOString(), model: 'gpt-4', request_count: 5, total_tokens: 100, last_message: 'Hello' },
      { id: '2', updated_at: new Date(Date.now()).toISOString(), model: 'gpt-4', request_count: 2, total_tokens: 50, last_message: 'Hi there' }
    ];

    await page.route('/api/conversations', async route => {
      if (route.request().method() === 'GET') {
        await route.fulfill({ json: mockConvs });
      } else {
        await route.continue();
      }
    });

    // Mock specific conversation load
    await page.route('/api/conversations/1', async route => {
      await route.fulfill({ json: {
        conversation: mockConvs[0],
        messages: [{ role: 'user', content: 'Hello', blocks: [], created_at: new Date().toISOString() }],
        context_files: []
      }});
    });

    await page.goto('/');
    
    // Check if list items appear
    await expect(page.locator('#conversation-list .list-item')).toHaveCount(2);
    await expect(page.locator('#conversation-list .list-item').first()).toContainText('Hello');
  });

  test('should create new conversation', async ({ page }) => {
    await page.route('/api/conversations', async route => {
      if (route.request().method() === 'GET') {
        await route.fulfill({ json: [] });
      } else if (route.request().method() === 'POST') {
        await route.fulfill({ json: { id: 'new-id', model: 'gpt-4' } });
      }
    });

    await page.route('/api/conversations/new-id', async route => {
      await route.fulfill({ json: {
        conversation: { id: 'new-id', created_at: new Date().toISOString(), updated_at: new Date().toISOString(), model: 'gpt-4', request_count: 0, total_tokens: 0 },
        messages: [],
        context_files: []
      }});
    });

    await page.goto('/');
    await page.click('#new-conversation');
    
    await expect(page.locator('#conversation-meta')).toHaveText('0 messages');
  });

  test('should send message', async ({ page }) => {
     // State to hold messages during the test interaction
     const conversationMessages: any[] = [];

     // Mock initial list
    await page.route('/api/conversations', async route => {
        await route.fulfill({ json: [{ id: '1', updated_at: new Date().toISOString(), model: 'gpt-4', request_count: 0, total_tokens: 0, last_message: null }] });
    });

    // Mock conversation load (GET) - returns the current state of messages
    await page.route('/api/conversations/1', async route => {
      await route.fulfill({ json: {
        conversation: { id: '1', updated_at: new Date().toISOString(), model: 'gpt-4', request_count: 0, total_tokens: 0 },
        messages: conversationMessages,
        context_files: []
      }});
    });

    // Mock stream response (POST)
    await page.route('/api/conversations/1/message/stream', async route => {
      const req = route.request().postDataJSON();
      
      // Simulate backend persisting the user message
      conversationMessages.push({
        role: 'user',
        content: req.message,
        blocks: [],
        created_at: new Date().toISOString()
      });

      const responseBody = [
        JSON.stringify({ type: 'text', delta: 'Hello ' }),
        JSON.stringify({ type: 'text', delta: 'World' })
      ].join('\n') + '\n';
      
      await route.fulfill ({ 
        body: Buffer.from(responseBody),
        headers: { 'Content-Type': 'application/x-ndjson' }
      });

      // Simulate backend persisting the assistant message
      conversationMessages.push({
        role: 'assistant',
        content: 'Hello World',
        blocks: [],
        created_at: new Date().toISOString()
      });
    });

    await page.goto('/');
    
    // Wait for conversation to load
    await expect(page.locator('#conversation-meta')).toContainText('messages');
    
    await page.fill('#message-input', 'Test Message');
    await page.click('#send-message');

    // Check if user message appears
    await expect(page.locator('.bubble.user')).toContainText('Test Message');
    
    // Check if assistant bubble exists
    await expect(page.locator('.bubble.assistant')).toBeVisible({ timeout: 10000 });

    // Check if assistant message appears (streamed)
    await expect(page.locator('.bubble.assistant')).toContainText('Hello World');
  });

  test('should lazy load conversations with pagination', async ({ page }) => {
    // Create 25 mock conversations to test pagination (default limit is 10)
    const mockConvsPage1 = Array.from({ length: 10 }, (_, i) => ({
      id: `${i + 1}`,
      updated_at: new Date(Date.now() - i * 1000).toISOString(),
      model: 'gpt-4',
      request_count: 1,
      total_tokens: 50,
      last_message: `Message ${i + 1}`
    }));

    const mockConvsPage2 = Array.from({ length: 10 }, (_, i) => ({
      id: `${i + 11}`,
      updated_at: new Date(Date.now() - (i + 10) * 1000).toISOString(),
      model: 'gpt-4',
      request_count: 1,
      total_tokens: 50,
      last_message: `Message ${i + 11}`
    }));

    const mockConvsPage3 = Array.from({ length: 5 }, (_, i) => ({
      id: `${i + 21}`,
      updated_at: new Date(Date.now() - (i + 20) * 1000).toISOString(),
      model: 'gpt-4',
      request_count: 1,
      total_tokens: 50,
      last_message: `Message ${i + 21}`
    }));

    await page.route('/api/conversations*', async route => {
      const url = new URL(route.request().url());
      const offset = parseInt(url.searchParams.get('offset') || '0');
      const limit = parseInt(url.searchParams.get('limit') || '10');

      if (offset === 0 && limit === 10) {
        await route.fulfill({ json: mockConvsPage1 });
      } else if (offset === 10 && limit === 10) {
        await route.fulfill({ json: mockConvsPage2 });
      } else if (offset === 20 && limit === 10) {
        await route.fulfill({ json: mockConvsPage3 });
      } else {
        await route.fulfill({ json: [] });
      }
    });

    // Mock specific conversation load for the first conversation
    await page.route('/api/conversations/1', async route => {
      await route.fulfill({ json: {
        conversation: mockConvsPage1[0],
        messages: [{ role: 'user', content: 'Message 1', blocks: [], created_at: new Date().toISOString() }],
        context_files: []
      }});
    });

    await page.goto('/');

    // Should load first 10 conversations
    await expect(page.locator('#conversation-list .list-item').filter({ hasNotText: 'Scroll to load more' })).toHaveCount(10);
    await expect(page.locator('#conversation-list .list-item').first()).toContainText('Message 1');

    // Should show "Scroll to load more" indicator
    await expect(page.locator('#conversation-list #conversation-load-more-spinner')).toContainText('Scroll to load more');

    // Scroll to bottom to trigger loading more
    await page.evaluate(() => {
      const list = document.getElementById('conversation-list');
      if (list) list.scrollTop = list.scrollHeight;
    });

    // Wait for second page to load
    await page.waitForTimeout(500);

    // Should now have 20 conversations
    await expect(page.locator('#conversation-list .list-item').filter({ hasNotText: 'Scroll to load more' })).toHaveCount(20);

    // Scroll again to load the third page
    await page.evaluate(() => {
      const list = document.getElementById('conversation-list');
      if (list) list.scrollTop = list.scrollHeight;
    });

    // Wait for third page to load
    await page.waitForTimeout(500);

    // Should now have 25 conversations (all loaded)
    await expect(page.locator('#conversation-list .list-item').filter({ hasNotText: 'Scroll to load more' })).toHaveCount(25);

    // Should not show "Scroll to load more" when all loaded (hasMore = false)
    await expect(page.locator('#conversation-list #conversation-load-more-spinner')).not.toBeVisible();
  });

  test('should show loading spinner while loading more conversations', async ({ page }) => {
    const mockConvsPage1 = Array.from({ length: 10 }, (_, i) => ({
      id: `${i + 1}`,
      updated_at: new Date(Date.now() - i * 1000).toISOString(),
      model: 'gpt-4',
      request_count: 1,
      total_tokens: 50,
      last_message: `Message ${i + 1}`
    }));

    let page2Requested = false;

    await page.route('/api/conversations*', async route => {
      const url = new URL(route.request().url());
      const offset = parseInt(url.searchParams.get('offset') || '0');

      if (offset === 0) {
        await route.fulfill({ json: mockConvsPage1 });
      } else if (offset === 10) {
        page2Requested = true;
        // Delay the response to check spinner visibility
        await new Promise(resolve => setTimeout(resolve, 1000));
        await route.fulfill({ json: [] });
      }
    });

    // Mock first conversation
    await page.route('/api/conversations/1', async route => {
      await route.fulfill({ json: {
        conversation: mockConvsPage1[0],
        messages: [{ role: 'user', content: 'Message 1', blocks: [], created_at: new Date().toISOString() }],
        context_files: []
      }});
    });

    await page.goto('/');

    // Wait for initial load
    await expect(page.locator('#conversation-list .list-item').filter({ hasNotText: 'Scroll to load more' })).toHaveCount(10);

    // Scroll to bottom to trigger loading more
    await page.evaluate(() => {
      const list = document.getElementById('conversation-list');
      if (list) list.scrollTop = list.scrollHeight;
    });

    // Wait a bit for the request to be initiated
    await page.waitForTimeout(100);

    // Should show loading spinner
    await expect(page.locator('#conversation-load-more-spinner .loading-spinner')).toBeVisible();

    // Wait for loading to complete
    await page.waitForTimeout(1100);

    // Spinner should be gone
    await expect(page.locator('#conversation-load-more-spinner')).not.toBeVisible();
  });
});