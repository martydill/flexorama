# Flexorama E2E Tests

This directory contains end-to-end integration tests for the Flexorama Web UI using Playwright.

## Prerequisites

1.  Node.js installed (v16+).
2.  Rust toolchain installed (to run the Flexorama backend).

## Setup

1.  Install dependencies:
    ```bash
    cd tests/e2e
    npm install
    npx playwright install --with-deps
    ```

## Running Tests

1.  Start the Flexorama backend in Web Mode in a separate terminal:
    ```bash
    # From the project root
    cargo run -- --web
    ```
    This will start the server on `http://localhost:3000` by default.

2.  Run the tests:
    ```bash
    cd tests/e2e
    npm test
    ```

    To run with UI:
    ```bash
    npm run test:ui
    ```

## Structure

*   `specs/`: Contains test files (*.spec.ts).
*   `playwright.config.ts`: Playwright configuration.
