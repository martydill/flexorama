import { Page } from '@playwright/test';
import * as fs from 'fs';
import * as path from 'path';

// Read files once
// We assume we are running from tests/e2e
const indexHtmlPath = path.resolve(__dirname, '../../../web/index.html');
const appJsPath = path.resolve(__dirname, '../../../web/app.js');

let indexHtml = fs.readFileSync(indexHtmlPath, 'utf-8');
const appJs = fs.readFileSync(appJsPath, 'utf-8');

// Inject dummy CSRF token
indexHtml = indexHtml.replace(
  "</head>",
  `<script>window.FLEXORAMA_CSRF_TOKEN = "test-token";</script></head>`
);

export async function setupAppMock(page: Page) {
  // Serve index.html (match root path with any query params)
  await page.route(url => url.pathname === '/', async route => {
    await route.fulfill({
      status: 200,
      contentType: 'text/html',
      body: indexHtml
    });
  });

  // Serve app.js
  await page.route('**/app.js', async route => {
    await route.fulfill({
      status: 200,
      contentType: 'application/javascript',
      body: appJs
    });
  });

  // Default API mocks to prevent bootstrap failures
  // Note: This only matches /api/conversations exactly, not /api/conversations/123
  await page.route('/api/conversations', async route => {
    if (route.request().method() === 'GET') {
      await route.fulfill({ json: [] });
    } else {
      // POST requests should be handled by test-specific mocks
      await route.fulfill({ status: 404, body: 'Not found' });
    }
  });
  await page.route('/api/models', async route => {
    await route.fulfill({ json: { provider: 'test', active_model: 'gpt-4', models: ['gpt-4'] } });
  });
  await page.route('/api/plans', async route => {
    if (route.request().method() === 'GET') await route.fulfill({ json: [] });
    else await route.continue();
  });
  await page.route('/api/mcp/servers', async route => {
    if (route.request().method() === 'GET') await route.fulfill({ json: [] });
    else await route.continue();
  });
  await page.route('/api/agents', async route => {
    if (route.request().method() === 'GET') await route.fulfill({ json: [] });
    else await route.continue();
  });
  await page.route('/api/agents/active', async route => {
    await route.fulfill({ json: { active: null } });
  });
  await page.route('/api/skills', async route => {
    if (route.request().method() === 'GET') await route.fulfill({ json: [] });
    else await route.continue();
  });
  await page.route('/api/commands', async route => {
    if (route.request().method() === 'GET') await route.fulfill({ json: [] });
    else await route.continue();
  });
  await page.route('/api/plan-mode', async route => {
    await route.fulfill({ json: { enabled: false } });
  });
  await page.route('/api/todos*', async route => {
    await route.fulfill({ json: [] });
  });
  await page.route('/api/permissions/pending*', async route => {
    await route.fulfill({ json: [] });
  });
  
  // Stats mocks
  await page.route('/api/stats/overview', async route => {
    await route.fulfill({ json: { total_conversations: 0, total_messages: 0, total_tokens: 0, total_requests: 0 } });
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

  // Serve any chart.js/highlight.js from CDN by allowing them or mocking them
  // The app uses CDN links. Playwright allows external requests by default unless mocked.
  // We should probably mock them for speed/reliability but letting them pass is fine for now.
}
