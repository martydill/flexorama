const express = require('express');
const cors = require('cors');
const path = require('path');
const db = require('./database');

const app = express();
const PORT = process.env.PORT || 3000;

// Middleware
app.use(cors());
app.use(express.json());
app.use(express.static('public'));

// API Routes

// Get all expenses
app.get('/api/expenses', (req, res) => {
  try {
    const expenses = db.prepare('SELECT * FROM expenses ORDER BY date DESC').all();
    res.json(expenses);
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

// Add a new expense
app.post('/api/expenses', (req, res) => {
  try {
    const { amount, category, description, date } = req.body;

    if (!amount || !category || !date) {
      return res.status(400).json({ error: 'Amount, category, and date are required' });
    }

    const stmt = db.prepare(`
      INSERT INTO expenses (amount, category, description, date)
      VALUES (?, ?, ?, ?)
    `);

    const result = stmt.run(amount, category, description || '', date);
    const newExpense = db.prepare('SELECT * FROM expenses WHERE id = ?').get(result.lastInsertRowid);

    res.status(201).json(newExpense);
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

// Delete an expense
app.delete('/api/expenses/:id', (req, res) => {
  try {
    const { id } = req.params;
    const stmt = db.prepare('DELETE FROM expenses WHERE id = ?');
    const result = stmt.run(id);

    if (result.changes === 0) {
      return res.status(404).json({ error: 'Expense not found' });
    }

    res.json({ message: 'Expense deleted successfully' });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

// Get expense summary
app.get('/api/summary', (req, res) => {
  try {
    // Total expenses
    const total = db.prepare('SELECT SUM(amount) as total FROM expenses').get();

    // By category
    const byCategory = db.prepare(`
      SELECT category, SUM(amount) as total, COUNT(*) as count
      FROM expenses
      GROUP BY category
      ORDER BY total DESC
    `).all();

    // By month (last 6 months)
    const byMonth = db.prepare(`
      SELECT
        strftime('%Y-%m', date) as month,
        SUM(amount) as total
      FROM expenses
      WHERE date >= date('now', '-6 months')
      GROUP BY strftime('%Y-%m', date)
      ORDER BY month DESC
    `).all();

    res.json({
      total: total.total || 0,
      byCategory,
      byMonth
    });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

// Serve the main page
app.get('/', (req, res) => {
  res.sendFile(path.join(__dirname, 'public', 'index.html'));
});

// Start server
app.listen(PORT, () => {
  console.log(`\n🚀 Expense Tracker server running on http://localhost:${PORT}`);
  console.log(`📊 Dashboard: http://localhost:${PORT}\n`);
});

module.exports = app;
