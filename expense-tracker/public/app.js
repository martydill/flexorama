// API Base URL
const API_URL = '/api';

// DOM Elements
const expenseForm = document.getElementById('expenseForm');
const expensesList = document.getElementById('expensesList');
const categoryBreakdown = document.getElementById('categoryBreakdown');
const totalAmount = document.getElementById('totalAmount');
const totalTransactions = document.getElementById('totalTransactions');

// Set default date to today
document.getElementById('date').valueAsDate = new Date();

// Format currency
function formatCurrency(amount) {
    return new Intl.NumberFormat('en-US', {
        style: 'currency',
        currency: 'USD'
    }).format(amount);
}

// Format date
function formatDate(dateString) {
    const date = new Date(dateString);
    return date.toLocaleDateString('en-US', {
        month: 'short',
        day: 'numeric',
        year: 'numeric'
    });
}

// Fetch all expenses
async function fetchExpenses() {
    try {
        const response = await fetch(`${API_URL}/expenses`);
        const expenses = await response.json();
        return expenses;
    } catch (error) {
        console.error('Error fetching expenses:', error);
        return [];
    }
}

// Fetch summary
async function fetchSummary() {
    try {
        const response = await fetch(`${API_URL}/summary`);
        const summary = await response.json();
        return summary;
    } catch (error) {
        console.error('Error fetching summary:', error);
        return { total: 0, byCategory: [], byMonth: [] };
    }
}

// Add expense
async function addExpense(expense) {
    try {
        const response = await fetch(`${API_URL}/expenses`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(expense)
        });
        if (!response.ok) {
            throw new Error('Failed to add expense');
        }
        return await response.json();
    } catch (error) {
        console.error('Error adding expense:', error);
        throw error;
    }
}

// Delete expense
async function deleteExpense(id) {
    try {
        const response = await fetch(`${API_URL}/expenses/${id}`, {
            method: 'DELETE'
        });
        if (!response.ok) {
            throw new Error('Failed to delete expense');
        }
        return await response.json();
    } catch (error) {
        console.error('Error deleting expense:', error);
        throw error;
    }
}

// Render expenses list
function renderExpenses(expenses) {
    if (expenses.length === 0) {
        expensesList.innerHTML = '<p class="empty-state">No expenses recorded yet.</p>';
        return;
    }

    expensesList.innerHTML = expenses.map(expense => `
        <div class="expense-item" data-id="${expense.id}">
            <div class="expense-info">
                <div class="expense-amount">${formatCurrency(expense.amount)}</div>
                <div class="expense-description">${expense.description || expense.category}</div>
                <div class="expense-meta">
                    <span>📂 ${expense.category}</span>
                    <span>📅 ${formatDate(expense.date)}</span>
                </div>
            </div>
            <div class="expense-actions">
                <button class="btn btn-delete" onclick="handleDelete(${expense.id})">
                    Delete
                </button>
            </div>
        </div>
    `).join('');
}

// Render category breakdown
function renderCategoryBreakdown(summary) {
    if (summary.byCategory.length === 0) {
        categoryBreakdown.innerHTML = '<p class="empty-state">No expenses yet. Add your first expense above!</p>';
        return;
    }

    categoryBreakdown.innerHTML = summary.byCategory.map(cat => `
        <div class="category-item">
            <div class="category-name">
                <span>${cat.category}</span>
            </div>
            <div class="category-amount">${formatCurrency(cat.total)}</div>
            <div class="category-count">${cat.count} transaction${cat.count !== 1 ? 's' : ''}</div>
        </div>
    `).join('');
}

// Render summary
function renderSummary(summary, expenses) {
    totalAmount.textContent = formatCurrency(summary.total);
    totalTransactions.textContent = expenses.length;
}

// Load and render all data
async function loadData() {
    const expenses = await fetchExpenses();
    const summary = await fetchSummary();

    renderExpenses(expenses);
    renderCategoryBreakdown(summary);
    renderSummary(summary, expenses);
}

// Handle form submission
expenseForm.addEventListener('submit', async (e) => {
    e.preventDefault();

    const expense = {
        amount: parseFloat(document.getElementById('amount').value),
        category: document.getElementById('category').value,
        description: document.getElementById('description').value,
        date: document.getElementById('date').value
    };

    try {
        await addExpense(expense);
        expenseForm.reset();
        document.getElementById('date').valueAsDate = new Date();
        await loadData();
    } catch (error) {
        alert('Failed to add expense. Please try again.');
    }
});

// Handle delete
async function handleDelete(id) {
    if (!confirm('Are you sure you want to delete this expense?')) {
        return;
    }

    try {
        await deleteExpense(id);
        await loadData();
    } catch (error) {
        alert('Failed to delete expense. Please try again.');
    }
}

// Make handleDelete available globally
window.handleDelete = handleDelete;

// Initial load
loadData();
