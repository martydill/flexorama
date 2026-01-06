use std::cell::Cell;
use std::thread_local;

/// Reverse search state
#[derive(Debug, Clone)]
pub struct ReverseSearchState {
    pub search_query: String,
    pub matched_entry: Option<String>,
    pub current_match_index: usize,
    pub all_matches: Vec<String>,
}

impl ReverseSearchState {
    pub fn new() -> Self {
        Self {
            search_query: String::new(),
            matched_entry: None,
            current_match_index: 0,
            all_matches: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.search_query.clear();
        self.matched_entry = None;
        self.current_match_index = 0;
        self.all_matches.clear();
    }
}

thread_local! {
    static LAST_RENDERED_LINES: Cell<usize> = Cell::new(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_history() {
        let mut history = InputHistory::new();

        // Test adding entries
        history.add_entry("test1".to_string());
        history.add_entry("test2".to_string());

        assert_eq!(history.entries.len(), 2);
        assert_eq!(history.entries[0], "test1");
        assert_eq!(history.entries[1], "test2");

        // Test navigation
        let current_input = "current";
        let prev = history.navigate_up(current_input);
        assert!(prev.is_some());
        assert_eq!(prev.unwrap(), "test2");

        let next = history.navigate_down();
        assert!(next.is_some());
        assert_eq!(next.unwrap(), current_input);

        // Test that empty entries are not added
        history.add_entry("".to_string());
        assert_eq!(history.entries.len(), 2); // Should not increase

        // Test that duplicates are not added
        history.add_entry("test2".to_string());
        assert_eq!(history.entries.len(), 2); // Should not increase
    }
}

/// Input history management with reverse search support
pub struct InputHistory {
    entries: Vec<String>,
    index: Option<usize>,
    temp_input: String,
    reverse_search: ReverseSearchState,
}

impl InputHistory {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: None,
            temp_input: String::new(),
            reverse_search: ReverseSearchState::new(),
        }
    }

    pub fn add_entry(&mut self, entry: String) {
        // Don't add empty entries or duplicates of the last entry
        if entry.trim().is_empty() {
            return;
        }

        if self.entries.is_empty() || self.entries.last() != Some(&entry) {
            self.entries.push(entry);
            // Limit history size to prevent memory issues
            if self.entries.len() > 1000 {
                self.entries.remove(0);
            }
        }

        // Reset navigation state
        self.index = None;
        self.temp_input.clear();
        self.reverse_search.reset();
    }

    pub fn navigate_up(&mut self, current_input: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }

        match self.index {
            None => {
                // First time pressing up - save current input and go to last entry
                self.temp_input = current_input.to_string();
                self.index = Some(self.entries.len() - 1);
                Some(self.entries[self.entries.len() - 1].clone())
            }
            Some(index) => {
                if index > 0 {
                    // Move to previous entry in history
                    self.index = Some(index - 1);
                    Some(self.entries[index - 1].clone())
                } else {
                    // Already at the oldest entry
                    Some(self.entries[0].clone())
                }
            }
        }
    }

    pub fn navigate_down(&mut self) -> Option<String> {
        match self.index {
            None => None,
            Some(index) => {
                if index < self.entries.len() - 1 {
                    // Move to next entry in history
                    self.index = Some(index + 1);
                    Some(self.entries[index + 1].clone())
                } else {
                    // At the end of history - restore current input
                    self.index = None;
                    Some(self.temp_input.clone())
                }
            }
        }
    }

    pub fn reset_navigation(&mut self) {
        self.index = None;
        self.temp_input.clear();
        self.reverse_search.reset();
    }

    /// Start reverse search mode
    pub fn start_reverse_search(&mut self, current_input: &str) {
        self.temp_input = current_input.to_string();
        self.reverse_search.reset();
        self.reverse_search.search_query = String::new();
        self.index = None;
    }

    /// Update reverse search query and find matches
    pub fn update_reverse_search(&mut self, query: &str) {
        self.reverse_search.search_query = query.to_string();

        if query.is_empty() {
            self.reverse_search.matched_entry = None;
            self.reverse_search.all_matches.clear();
            self.reverse_search.current_match_index = 0;
            return;
        }

        // Find all entries that contain the query (case-insensitive)
        let query_lower = query.to_lowercase();
        self.reverse_search.all_matches = self
            .entries
            .iter()
            .rev() // Start from most recent
            .filter(|entry| entry.to_lowercase().contains(&query_lower))
            .cloned()
            .collect();

        if !self.reverse_search.all_matches.is_empty() {
            self.reverse_search.current_match_index = 0;
            self.reverse_search.matched_entry = Some(self.reverse_search.all_matches[0].clone());
        } else {
            self.reverse_search.matched_entry = None;
        }
    }

    /// Navigate to next match in reverse search
    pub fn reverse_search_next(&mut self) {
        if !self.reverse_search.all_matches.is_empty() {
            self.reverse_search.current_match_index = (self.reverse_search.current_match_index + 1)
                % self.reverse_search.all_matches.len();
            self.reverse_search.matched_entry = Some(
                self.reverse_search.all_matches[self.reverse_search.current_match_index].clone(),
            );
        }
    }

    /// Navigate to previous match in reverse search
    pub fn reverse_search_prev(&mut self) {
        if !self.reverse_search.all_matches.is_empty() {
            self.reverse_search.current_match_index =
                if self.reverse_search.current_match_index == 0 {
                    self.reverse_search.all_matches.len() - 1
                } else {
                    self.reverse_search.current_match_index - 1
                };
            self.reverse_search.matched_entry = Some(
                self.reverse_search.all_matches[self.reverse_search.current_match_index].clone(),
            );
        }
    }

    /// Get the current reverse search state
    pub fn get_reverse_search_state(&self) -> &ReverseSearchState {
        &self.reverse_search
    }

    /// Finish reverse search and return the matched entry
    pub fn finish_reverse_search(&mut self) -> Option<String> {
        let result = self.reverse_search.matched_entry.clone();
        self.reverse_search.reset();
        self.index = None;
        result
    }

    /// Cancel reverse search and restore original input
    pub fn cancel_reverse_search(&mut self) -> String {
        self.reverse_search.reset();
        self.index = None;
        self.temp_input.clone()
    }
}

