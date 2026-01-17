import { test, expect } from '@playwright/test';
import { setupAppMock } from '../lib/mocks';

test.describe('Skills', () => {
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
     await page.route('/api/skills/active', async route => {
        await route.fulfill({ json: [] });
    });
  });

  test('should list skills', async ({ page }) => {
    const mockSkills = [
      { name: 'Coding', description: 'Coding skill', active: true, allowed_tools: [], denied_tools: [], tags: [], references: [] },
      { name: 'Writing', description: 'Writing skill', active: false, allowed_tools: [], denied_tools: [], tags: [], references: [] }
    ];

    await page.route('/api/skills', async route => {
      await route.fulfill({ json: mockSkills });
    });

    await page.goto('/?tab=skills');
    
    await expect(page.locator('#skill-list .list-item')).toHaveCount(2);
    // Active skill indicator
    await expect(page.locator('#skill-list .list-item').first()).toContainText('🟢');
  });

  test('should create skill', async ({ page }) => {
    await page.route('/api/skills', async route => {
        if (route.request().method() === 'GET') await route.fulfill({ json: [] });
        else await route.fulfill({ json: { name: 'NewSkill' } });
    });

    await page.goto('/?tab=skills');
    await page.click('#new-skill');
    
    await page.fill('#skill-name', 'NewSkill');
    await page.fill('#skill-description', 'A new skill');
    await page.fill('#skill-content', 'Skill content');
    
    await page.click('#save-skill');
  });

  test('should update skill', async ({ page }) => {
    const skill = { name: 'Coding', description: 'Old desc', active: true, allowed_tools: [], denied_tools: [], tags: [], references: [], content: 'Content' };
    await page.route('/api/skills', async route => {
        await route.fulfill({ json: [skill] });
    });

    let updateCalled = false;
    await page.route('/api/skills/Coding', async route => {
        if (route.request().method() === 'PUT') {
            updateCalled = true;
            const body = route.request().postDataJSON();
            expect(body.description).toBe('New desc');
            await route.fulfill({ json: { ...skill, ...body } });
        } else {
            await route.continue();
        }
    });

    await page.goto('/?tab=skills');
    await page.click('#skill-list .list-item');
    await page.fill('#skill-description', 'New desc');
    await page.click('#save-skill');
    
    expect(updateCalled).toBe(true);
  });

  test('should delete skill', async ({ page }) => {
    const skill = { name: 'Coding', description: 'Desc', active: true, allowed_tools: [], denied_tools: [], tags: [], references: [], content: 'Content' };
    await page.route('/api/skills', async route => {
        await route.fulfill({ json: [skill] });
    });

    let deleteCalled = false;
    await page.route('/api/skills/Coding', async route => {
        if (route.request().method() === 'DELETE') {
            deleteCalled = true;
            await route.fulfill({ json: { success: true } });
        }
    });

    await page.goto('/?tab=skills');
    await page.click('#skill-list .list-item');
    
    await expect(page.locator('#delete-skill')).toBeVisible();
    await page.click('#delete-skill');
    
    expect(deleteCalled).toBe(true);
  });
});
