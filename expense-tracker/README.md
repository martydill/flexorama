# 💰 Expense Tracker

A full-stack web application for tracking personal expenses with a clean, modern interface.

## Features

- ✅ Add expenses with amount, category, description, and date
- 📊 View spending by category with visual breakdown
- 🗑️ Delete expenses
- 📈 Real-time summary statistics
- 🎨 Beautiful, responsive UI
- 💾 SQLite database for data persistence

## Tech Stack

- **Backend**: Node.js + Express
- **Database**: SQLite (better-sqlite3)
- **Frontend**: HTML, CSS, JavaScript (Vanilla)
- **API**: RESTful endpoints

## Installation

1. Install dependencies:
```bash
npm install
```

2. Start the server:
```bash
npm start
```

3. Open your browser:
```
http://localhost:3000
```

## API Endpoints

- `GET /api/expenses` - Get all expenses
- `POST /api/expenses` - Add a new expense
- `DELETE /api/expenses/:id` - Delete an expense
- `GET /api/summary` - Get expense summary statistics

## Usage

1. **Add an Expense**:
   - Enter the amount
   - Select a category (Food & Dining, Transportation, Shopping, etc.)
   - Add a description (optional)
   - Select the date
   - Click "Add Expense"

2. **View Expenses**:
   - See all expenses in the "Recent Expenses" section
   - View spending breakdown by category
   - Check total expenses and transaction count

3. **Delete an Expense**:
   - Click the "Delete" button on any expense item
   - Confirm the deletion

## Project Structure

```
expense-tracker/
├── server.js           # Express server and API routes
├── database.js         # SQLite database setup
├── package.json        # Dependencies and scripts
├── expenses.db         # SQLite database (created at runtime)
└── public/
    ├── index.html      # Main UI
    ├── style.css       # Styling
    └── app.js          # Frontend logic
```

## Development

For development with auto-reload:
```bash
npm run dev
```

## Database Schema

```sql
CREATE TABLE expenses (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    amount DECIMAL(10, 2) NOT NULL,
    category TEXT NOT NULL,
    description TEXT,
    date TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
)
```

## License

MIT
