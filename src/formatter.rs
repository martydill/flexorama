use anyhow::Result;
use colored::*;
use regex::Regex;

pub struct CodeFormatter {
    code_block_regex: Regex,
    file_regex: Regex,
    number_regex: Regex,
    cache: std::cell::RefCell<InputHighlightCache>,
}

#[derive(Clone)]
struct InputHighlightCache {
    last_input: String,
    last_result: String,
}

impl CodeFormatter {
    pub fn new() -> Result<Self> {
        let code_block_regex = Regex::new(r"```(\w*)\n([\s\S]*?)```")?;
        let file_regex = Regex::new(r"@([^\s@]+)")?;
        let number_regex = Regex::new(r"\b\d+(\.\d+)?\b")?;

        Ok(Self {
            code_block_regex,
            file_regex,
            number_regex,
            cache: std::cell::RefCell::new(InputHighlightCache {
                last_input: String::new(),
                last_result: String::new(),
            }),
        })
    }

    pub fn format_response(&self, response: &str) -> Result<String> {
        let formatted = self.format_text_with_code_blocks(response)?;
        Ok(formatted)
    }

    /// Format response with file highlighting (for user input display) with caching
    pub fn format_input_with_file_highlighting(&self, input: &str) -> String {
        // First do a cheap check - if no @ symbol, return as-is (fast path)
        if !input.contains('@') {
            return input.to_string();
        }

        let mut cache = self.cache.borrow_mut();

        // Return cached result if input hasn't changed
        if cache.last_input == input {
            return cache.last_result.clone();
        }

        // Compute new result and cache it
        let result = self.format_text_with_file_highlighting(input);
        cache.last_input = input.to_string();
        cache.last_result = result.clone();

        result
    }

    fn format_text_with_code_blocks(&self, text: &str) -> Result<String> {
        let mut result = String::new();
        let mut last_end = 0;

        for caps in self.code_block_regex.captures_iter(text) {
            let full_match = caps.get(0).unwrap();
            let lang = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let code = caps.get(2).unwrap().as_str();

            // Add text before the code block
            result.push_str(&text[last_end..full_match.start()]);

            // Format and add the code block
            let formatted_code = self.format_code_block(code, lang)?;
            result.push_str(&formatted_code);

            last_end = full_match.end();
        }

        // Add remaining text after the last code block
        result.push_str(&text[last_end..]);

        Ok(result)
    }

    fn format_code_block(&self, code: &str, lang: &str) -> Result<String> {
        let mut result = String::new();

        // Normalize language name
        let normalized_lang = self.normalize_language(lang);

        // Add header with language info
        result.push_str(&self.build_code_block_header(normalized_lang));
        result.push('\n');

        // Add code content with syntax highlighting
        for line in code.lines() {
            let highlighted_line = self.highlight_line(line, normalized_lang);
            result.push_str(&highlighted_line);
            result.push('\n');
        }

        // Add footer
        result.push_str(&self.build_code_block_footer(normalized_lang));
        result.push('\n');

        Ok(result)
    }

    fn build_code_block_header(&self, normalized_lang: &str) -> String {
        format!(
            "{}{} {} {}{}",
            "┌".bold().cyan(),
            " ".repeat(2),
            normalized_lang.to_uppercase().bold().white(),
            " ".repeat(2),
            "┐".bold().cyan()
        )
    }

    fn build_code_block_footer(&self, normalized_lang: &str) -> String {
        let footer_width = normalized_lang.len() + 6;
        format!(
            "{}{}{}",
            "└".bold().cyan(),
            "─".repeat(footer_width).cyan(),
            "┘".bold().cyan()
        )
    }

    fn highlight_line(&self, line: &str, lang: &str) -> String {
        match lang {
            "rust" => self.highlight_rust(line),
            "python" => self.highlight_python(line),
            "javascript" | "js" | "jsx" => self.highlight_javascript(line),
            "typescript" | "ts" | "tsx" => self.highlight_typescript(line),
            "json" => self.highlight_json(line),
            "yaml" | "yml" => self.highlight_yaml(line),
            "html" => self.highlight_html(line),
            "css" => self.highlight_css(line),
            "bash" | "sh" => self.highlight_bash(line),
            "sql" => self.highlight_sql(line),
            "markdown" | "md" => self.highlight_markdown(line),
            "toml" => self.highlight_toml(line),
            "xml" => self.highlight_xml(line),
            "c" | "cpp" | "c++" => self.highlight_c_cpp(line),
            "java" => self.highlight_java(line),
            "go" => self.highlight_go(line),
            _ => self.highlight_numbers(line),
        }
    }

    // Basic syntax highlighting for various languages
    fn highlight_rust(&self, line: &str) -> String {
        let mut result = self.highlight_numbers(line);

        // Keywords
        let keywords = [
            "fn", "let", "mut", "const", "static", "if", "else", "match", "for", "while", "loop",
            "break", "continue", "return", "struct", "enum", "impl", "trait", "mod", "use", "pub",
            "crate", "super", "self", "Self", "where", "async", "await", "move", "ref", "unsafe",
            "extern",
        ];
        for keyword in &keywords {
            let regex = Regex::new(&format!(r"\b{}\b", regex::escape(keyword))).unwrap();
            result = regex
                .replace_all(&result, keyword.bold().blue().to_string())
                .to_string();
        }

        // Types
        let types = [
            "String", "str", "Vec", "Option", "Result", "Box", "Rc", "Arc", "Cell", "RefCell",
            "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128", "f32", "f64",
            "bool", "char",
        ];
        for type_name in &types {
            let regex = Regex::new(&format!(r"\b{}\b", regex::escape(type_name))).unwrap();
            result = regex
                .replace_all(&result, type_name.bold().cyan().to_string())
                .to_string();
        }

        // Strings
        let string_regex = Regex::new(r#""([^"\\]|\\.)*""#).unwrap();
        result = string_regex
            .replace_all(&result, |caps: &regex::Captures| {
                format!("\"{}\"", &caps[0][1..caps[0].len() - 1].green())
            })
            .to_string();

        // Comments
        if result.starts_with("//") {
            result = result.dimmed().to_string();
        } else if let Some(pos) = result.find("//") {
            let (before, after) = result.split_at(pos);
            result = format!("{}{}", before, after.dimmed());
        }

        result
    }

    fn highlight_python(&self, line: &str) -> String {
        let mut result = self.highlight_numbers(line);

        // Keywords
        let keywords = [
            "def", "class", "if", "elif", "else", "for", "while", "try", "except", "finally",
            "with", "as", "import", "from", "return", "yield", "lambda", "and", "or", "not", "in",
            "is", "None", "True", "False", "pass", "break", "continue", "global", "nonlocal",
            "async", "await",
        ];
        for keyword in &keywords {
            let regex = Regex::new(&format!(r"\b{}\b", regex::escape(keyword))).unwrap();
            result = regex
                .replace_all(&result, keyword.bold().blue().to_string())
                .to_string();
        }

        // Strings
        let string_regex =
            Regex::new(r#"'([^'\\]|\\.)*'|"""([^"\\]|\\.)*"""|"([^"\\]|\\.)*"|"""([^"\\]|\\.)*"""#)
                .unwrap();
        result = string_regex
            .replace_all(&result, |caps: &regex::Captures| {
                caps[0].green().to_string()
            })
            .to_string();

        // Comments
        if result.starts_with("#") {
            result = result.dimmed().to_string();
        } else if let Some(pos) = result.find("#") {
            let (before, after) = result.split_at(pos);
            result = format!("{}{}", before, after.dimmed());
        }

        result
    }

    fn highlight_javascript(&self, line: &str) -> String {
        let mut result = self.highlight_numbers(line);

        // Keywords
        let keywords = [
            "function",
            "const",
            "let",
            "var",
            "if",
            "else",
            "for",
            "while",
            "do",
            "switch",
            "case",
            "default",
            "break",
            "continue",
            "return",
            "try",
            "catch",
            "finally",
            "throw",
            "new",
            "this",
            "typeof",
            "instanceof",
            "in",
            "of",
            "class",
            "extends",
            "super",
            "static",
            "async",
            "await",
            "import",
            "export",
            "from",
            "default",
        ];
        for keyword in &keywords {
            let regex = Regex::new(&format!(r"\b{}\b", regex::escape(keyword))).unwrap();
            result = regex
                .replace_all(&result, keyword.bold().blue().to_string())
                .to_string();
        }

        // Strings
        let string_regex =
            Regex::new(r#"'([^'\\]|\\.)*'|"(?:"([^"\\]|\\.)*")|`([^`\\]|\\.)*`"#).unwrap();
        result = string_regex
            .replace_all(&result, |caps: &regex::Captures| {
                caps[0].green().to_string()
            })
            .to_string();

        // Comments
        if result.starts_with("//") {
            result = result.dimmed().to_string();
        } else if let Some(pos) = result.find("//") {
            let (before, after) = result.split_at(pos);
            result = format!("{}{}", before, after.dimmed());
        }

        result
    }

    fn highlight_typescript(&self, line: &str) -> String {
        // TypeScript is similar to JavaScript but with additional type keywords
        let mut result = self.highlight_javascript(line);

        // TypeScript specific keywords
        let ts_keywords = [
            "interface",
            "type",
            "enum",
            "namespace",
            "module",
            "declare",
            "abstract",
            "readonly",
            "private",
            "public",
            "protected",
            "implements",
            "keyof",
            "unknown",
            "never",
            "any",
        ];
        for keyword in &ts_keywords {
            let regex = Regex::new(&format!(r"\b{}\b", regex::escape(keyword))).unwrap();
            result = regex
                .replace_all(&result, keyword.bold().magenta().to_string())
                .to_string();
        }

        result
    }

    fn highlight_json(&self, line: &str) -> String {
        let mut result = line.to_string();

        // JSON keys (strings before colons)
        let key_regex = Regex::new(r#""([^"\\]|\\.)*"\s*:"#).unwrap();
        result = key_regex
            .replace_all(&result, |caps: &regex::Captures| {
                let key_part = &caps[0][..caps[0].len() - 1];
                format!("{}:", key_part.bold().cyan())
            })
            .to_string();

        // JSON string values
        let string_regex = Regex::new(r#":\s*"([^"\\]|\\.)*""#).unwrap();
        result = string_regex
            .replace_all(&result, |caps: &regex::Captures| {
                format!(": {}", &caps[0][2..].green())
            })
            .to_string();

        // JSON numbers and booleans
        let value_regex = Regex::new(r":\s*(true|false|null|\d+\.?\d*)").unwrap();
        result = value_regex
            .replace_all(&result, |caps: &regex::Captures| {
                format!(": {}", &caps[0][2..].yellow())
            })
            .to_string();

        result
    }

    fn highlight_yaml(&self, line: &str) -> String {
        let mut result = line.to_string();

        // YAML keys (before colons)
        if let Some(colon_pos) = result.find(':') {
            let (key, rest) = result.split_at(colon_pos);
            result = format!("{}{}", key.bold().cyan(), rest);
        }

        // YAML string values
        let string_regex = Regex::new(r#":\s*["'][^"']*["']"#).unwrap();
        result = string_regex
            .replace_all(&result, |caps: &regex::Captures| {
                format!(": {}", &caps[0][2..].green())
            })
            .to_string();

        // YAML numbers and booleans
        let value_regex = Regex::new(r":\s*(true|false|null|\d+\.?\d*)").unwrap();
        result = value_regex
            .replace_all(&result, |caps: &regex::Captures| {
                format!(": {}", &caps[0][2..].yellow())
            })
            .to_string();

        result
    }

    fn highlight_html(&self, line: &str) -> String {
        let mut result = self.highlight_numbers(line);

        // HTML tags
        let tag_regex = Regex::new(r"</?[^>]+>").unwrap();
        result = tag_regex
            .replace_all(&result, |caps: &regex::Captures| caps[0].blue().to_string())
            .to_string();

        // HTML attributes
        let attr_regex = Regex::new(r#"(\w+)=["'][^"']*["']"#).unwrap();
        result = attr_regex
            .replace_all(&result, |caps: &regex::Captures| {
                format!("{}={}", caps[1].cyan(), &caps[0][caps[1].len()..].green())
            })
            .to_string();

        result
    }

    fn highlight_css(&self, line: &str) -> String {
        let mut result = self.highlight_numbers(line);

        // CSS selectors and properties
        let selector_regex = Regex::new(r"[.#]?[\w-]+\s*\{").unwrap();
        result = selector_regex
            .replace_all(&result, |caps: &regex::Captures| {
                caps[0].bold().blue().to_string()
            })
            .to_string();

        // CSS properties
        let prop_regex = Regex::new(r"[\w-]+:").unwrap();
        result = prop_regex
            .replace_all(&result, |caps: &regex::Captures| caps[0].cyan().to_string())
            .to_string();

        // CSS values
        let value_regex = Regex::new(r":\s*[^;]+;?").unwrap();
        result = value_regex
            .replace_all(&result, |caps: &regex::Captures| {
                format!(": {}", &caps[0][2..].green())
            })
            .to_string();

        result
    }

    fn highlight_bash(&self, line: &str) -> String {
        let mut result = self.highlight_numbers(line);

        // Bash commands
        let commands = [
            "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac",
            "function", "return", "exit", "export", "local", "readonly", "declare", "typeset",
            "alias", "unalias", "cd", "pwd", "ls", "mkdir", "rmdir", "rm", "cp", "mv", "ln", "cat",
            "less", "more", "head", "tail", "grep", "sed", "awk", "sort", "uniq", "wc", "find",
            "locate", "which", "whereis", "man", "echo", "printf", "read", "trap", "wait", "jobs",
            "fg", "bg", "kill", "ps", "top", "df", "du", "free", "uname", "uptime", "date", "cal",
            "tar", "gzip", "gunzip", "zip", "unzip", "ssh", "scp", "rsync", "git", "make", "gcc",
            "g++", "python", "python3", "node", "npm", "yarn", "docker", "kubectl",
        ];
        for command in &commands {
            let regex = Regex::new(&format!(r"\b{}\b", regex::escape(command))).unwrap();
            result = regex
                .replace_all(&result, command.bold().green().to_string())
                .to_string();
        }

        // Strings
        let string_regex = Regex::new(r#"'([^'\\]|\\.)*'|"(?:[^"\\]|\\.)*""#).unwrap();
        result = string_regex
            .replace_all(&result, |caps: &regex::Captures| {
                caps[0].yellow().to_string()
            })
            .to_string();

        // Comments
        if result.starts_with("#") {
            result = result.dimmed().to_string();
        }

        result
    }

    fn highlight_sql(&self, line: &str) -> String {
        let mut result = self.highlight_numbers(line);

        // SQL keywords
        let keywords = [
            "SELECT",
            "FROM",
            "WHERE",
            "INSERT",
            "UPDATE",
            "DELETE",
            "CREATE",
            "ALTER",
            "DROP",
            "TABLE",
            "INDEX",
            "DATABASE",
            "SCHEMA",
            "PRIMARY",
            "FOREIGN",
            "KEY",
            "REFERENCES",
            "JOIN",
            "INNER",
            "LEFT",
            "RIGHT",
            "FULL",
            "OUTER",
            "ON",
            "GROUP",
            "BY",
            "ORDER",
            "HAVING",
            "LIMIT",
            "OFFSET",
            "UNION",
            "ALL",
            "DISTINCT",
            "COUNT",
            "SUM",
            "AVG",
            "MIN",
            "MAX",
            "AND",
            "OR",
            "NOT",
            "IN",
            "EXISTS",
            "BETWEEN",
            "LIKE",
            "ILIKE",
            "NULL",
            "IS",
            "AS",
            "CASE",
            "WHEN",
            "THEN",
            "ELSE",
            "END",
            "IF",
            "COALESCE",
            "CAST",
            "CONVERT",
            "TRY_CAST",
            "TRY_CONVERT",
        ];
        for keyword in &keywords {
            let regex = Regex::new(&format!(r"\b{}\b", regex::escape(keyword))).unwrap();
            result = regex
                .replace_all(&result, keyword.bold().blue().to_string())
                .to_string();
        }

        // SQL identifiers (backticks, brackets, quotes)
        let ident_regex = Regex::new(r#"[`'"\[\]]([^`'"\[\]]*)[`'"\[\]]"#).unwrap();
        result = ident_regex
            .replace_all(&result, |caps: &regex::Captures| caps[0].cyan().to_string())
            .to_string();

        result
    }

    fn highlight_markdown(&self, line: &str) -> String {
        let mut result = self.highlight_numbers(line);

        // Headers
        if result.starts_with('#') {
            let header_level = result.chars().take_while(|&c| c == '#').count();
            let remaining = &result[header_level..];
            result = format!(
                "{}{}",
                "#".repeat(header_level).bold().red(),
                remaining.bold()
            );
        }

        // Bold text
        let bold_regex = Regex::new(r"\*\*([^*]+)\*\*").unwrap();
        result = bold_regex
            .replace_all(&result, |caps: &regex::Captures| caps[0].bold().to_string())
            .to_string();

        // Italic text
        let italic_regex = Regex::new(r"\*([^*]+)\*").unwrap();
        result = italic_regex
            .replace_all(&result, |caps: &regex::Captures| {
                caps[0].italic().to_string()
            })
            .to_string();

        // Code inline
        let code_regex = Regex::new(r"`([^`]+)`").unwrap();
        result = code_regex
            .replace_all(&result, |caps: &regex::Captures| {
                format!("`{}`", caps[1].black().on_white())
            })
            .to_string();

        // Links
        let link_regex = Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap();
        result = link_regex
            .replace_all(&result, |caps: &regex::Captures| {
                format!("[{}]({})", caps[1].blue().underline(), caps[2].dimmed())
            })
            .to_string();

        result
    }

    fn highlight_toml(&self, line: &str) -> String {
        let mut result = self.highlight_numbers(line);

        // TOML sections
        if result.starts_with('[') && result.ends_with(']') {
            result = result.bold().blue().to_string();
        }

        // TOML keys
        if let Some(eq_pos) = result.find('=') {
            let (key, rest) = result.split_at(eq_pos);
            result = format!("{}{}", key.cyan(), rest);
        }

        // TOML strings
        let string_regex = Regex::new(r#"="([^"\\]|\\.)*"|'([^'\\]|\\.)*'"#).unwrap();
        result = string_regex
            .replace_all(&result, |caps: &regex::Captures| {
                caps[0].green().to_string()
            })
            .to_string();

        result
    }

    fn highlight_xml(&self, line: &str) -> String {
        self.highlight_html(line) // XML highlighting is similar to HTML
    }

    fn highlight_c_cpp(&self, line: &str) -> String {
        let mut result = self.highlight_numbers(line);

        // C/C++ keywords
        let keywords = [
            "int",
            "char",
            "float",
            "double",
            "void",
            "long",
            "short",
            "unsigned",
            "signed",
            "const",
            "static",
            "extern",
            "auto",
            "register",
            "volatile",
            "sizeof",
            "typedef",
            "struct",
            "union",
            "enum",
            "if",
            "else",
            "for",
            "while",
            "do",
            "switch",
            "case",
            "default",
            "break",
            "continue",
            "return",
            "goto",
            "include",
            "define",
            "ifdef",
            "ifndef",
            "endif",
            "class",
            "public",
            "private",
            "protected",
            "virtual",
            "inline",
            "friend",
            "operator",
            "new",
            "delete",
            "this",
            "namespace",
            "using",
            "template",
            "typename",
        ];
        for keyword in &keywords {
            let regex = Regex::new(&format!(r"\b{}\b", regex::escape(keyword))).unwrap();
            result = regex
                .replace_all(&result, keyword.bold().blue().to_string())
                .to_string();
        }

        // Preprocessor directives
        if result.starts_with('#') {
            result = result.bold().magenta().to_string();
        }

        // Comments
        if result.starts_with("//") {
            result = result.dimmed().to_string();
        } else if result.starts_with("/*") || result.contains("*/") {
            result = result.dimmed().to_string();
        }

        result
    }

    fn highlight_java(&self, line: &str) -> String {
        let mut result = self.highlight_numbers(line);

        // Java keywords
        let keywords = [
            "public",
            "private",
            "protected",
            "static",
            "final",
            "abstract",
            "synchronized",
            "volatile",
            "transient",
            "native",
            "strictfp",
            "class",
            "interface",
            "extends",
            "implements",
            "import",
            "package",
            "if",
            "else",
            "for",
            "while",
            "do",
            "switch",
            "case",
            "default",
            "break",
            "continue",
            "return",
            "throw",
            "throws",
            "try",
            "catch",
            "finally",
            "new",
            "this",
            "super",
            "null",
            "true",
            "false",
            "instanceof",
            "enum",
            "assert",
        ];
        for keyword in &keywords {
            let regex = Regex::new(&format!(r"\b{}\b", regex::escape(keyword))).unwrap();
            result = regex
                .replace_all(&result, keyword.bold().blue().to_string())
                .to_string();
        }

        // Annotations
        if result.starts_with('@') {
            result = result.bold().magenta().to_string();
        }

        result
    }

    fn highlight_go(&self, line: &str) -> String {
        let mut result = self.highlight_numbers(line);

        // Go keywords
        let keywords = [
            "break",
            "case",
            "chan",
            "const",
            "continue",
            "default",
            "defer",
            "else",
            "fallthrough",
            "for",
            "func",
            "go",
            "goto",
            "if",
            "import",
            "interface",
            "map",
            "package",
            "range",
            "return",
            "select",
            "struct",
            "switch",
            "type",
            "var",
        ];
        for keyword in &keywords {
            let regex = Regex::new(&format!(r"\b{}\b", regex::escape(keyword))).unwrap();
            result = regex
                .replace_all(&result, keyword.bold().blue().to_string())
                .to_string();
        }

        // Go types
        let types = [
            "int",
            "int8",
            "int16",
            "int32",
            "int64",
            "uint",
            "uint8",
            "uint16",
            "uint32",
            "uint64",
            "float32",
            "float64",
            "complex64",
            "complex128",
            "bool",
            "string",
            "byte",
            "rune",
        ];
        for type_name in &types {
            let regex = Regex::new(&format!(r"\b{}\b", regex::escape(type_name))).unwrap();
            result = regex
                .replace_all(&result, type_name.bold().cyan().to_string())
                .to_string();
        }

        result
    }

    fn highlight_numbers(&self, text: &str) -> String {
        self.number_regex
            .replace_all(text, |caps: &regex::Captures| caps[0].yellow().to_string())
            .to_string()
    }

    fn normalize_language<'a>(&self, lang: &'a str) -> &'a str {
        match lang.to_lowercase().as_str() {
            "js" => "javascript",
            "ts" => "typescript",
            "jsx" => "javascript",
            "tsx" => "typescript",
            "py" => "python",
            "rb" => "ruby",
            "sh" | "bash" | "zsh" => "bash",
            "yml" => "yaml",
            "rs" => "rust",
            "c" => "c",
            "cpp" | "cxx" | "cc" => "cpp",
            "md" => "markdown",
            "" => "text",
            _ => lang,
        }
    }

    pub fn print_formatted(&self, text: &str) -> Result<()> {
        let formatted = self.format_response(text)?;
        app_print!("{}", formatted);
        crate::output::flush();
        Ok(())
    }

    /// Format text with @file syntax highlighting (standalone function)
    pub fn format_text_with_file_highlighting(&self, text: &str) -> String {
        let mut result = String::new();
        let mut last_end = 0;

        // Find all @file references
        for caps in self.file_regex.captures_iter(text) {
            let full_match = caps.get(0).unwrap();
            let file_path = caps.get(1).unwrap().as_str();

            // Add text before the file reference
            result.push_str(&text[last_end..full_match.start()]);

            // Add highlighted file reference with background color
            let highlighted_file = format!("@{file_path}")
                .on_bright_blue()
                .white()
                .bold()
                .to_string();
            result.push_str(&highlighted_file);

            last_end = full_match.end();
        }

        // Add remaining text after the last file reference
        result.push_str(&text[last_end..]);

        result
    }
}

pub fn create_code_formatter() -> Result<CodeFormatter> {
    CodeFormatter::new()
}

impl Clone for CodeFormatter {
    fn clone(&self) -> Self {
        Self {
            code_block_regex: self.code_block_regex.clone(),
            file_regex: self.file_regex.clone(),
            number_regex: self.number_regex.clone(),
            cache: std::cell::RefCell::new(self.cache.borrow().clone()),
        }
    }
}

pub struct StreamingResponseFormatter {
    formatter: CodeFormatter,
    pending_line: String,
    in_code_block: bool,
    current_lang: String,
}

impl StreamingResponseFormatter {
    pub fn new(formatter: CodeFormatter) -> Self {
        Self {
            formatter,
            pending_line: String::new(),
            in_code_block: false,
            current_lang: "text".to_string(),
        }
    }

    pub fn handle_chunk(&mut self, chunk: &str) -> Result<()> {
        if chunk.is_empty() {
            return Ok(());
        }

        self.pending_line.push_str(chunk);

        while let Some(pos) = self.pending_line.find('\n') {
            let line = self.pending_line[..pos].to_string();
            self.pending_line.drain(..=pos);
            self.process_complete_line(line.trim_end_matches('\r'))?;
        }

        crate::output::flush();
        Ok(())
    }

    pub fn finish(&mut self) -> Result<()> {
        if !self.pending_line.is_empty() {
            if self.in_code_block {
                let highlighted = self
                    .formatter
                    .highlight_line(&self.pending_line, &self.current_lang);
                app_println!("{}", highlighted);
            } else {
                app_print!("{}", self.pending_line);
            }
            self.pending_line.clear();
        }

        if self.in_code_block {
            app_println!(
                "{}",
                self.formatter.build_code_block_footer(&self.current_lang)
            );
            self.in_code_block = false;
        }

        crate::output::flush();
        Ok(())
    }

    fn process_complete_line(&mut self, line: &str) -> Result<()> {
        if self.in_code_block {
            self.handle_code_line(line);
        } else {
            self.handle_plain_line(line)?;
        }
        Ok(())
    }

    fn handle_plain_line(&mut self, line: &str) -> Result<()> {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            let lang = trimmed.trim_start_matches("```").trim();
            self.start_code_block(lang);
        } else {
            app_println!("{}", line);
        }
        Ok(())
    }

    fn handle_code_line(&mut self, line: &str) {
        if line.trim() == "```" {
            app_println!(
                "{}",
                self.formatter.build_code_block_footer(&self.current_lang)
            );
            self.in_code_block = false;
            return;
        }

        let highlighted = self.formatter.highlight_line(line, &self.current_lang);
        app_println!("{}", highlighted);
    }

    fn start_code_block(&mut self, lang: &str) {
        let normalized = self.formatter.normalize_language(lang);
        let language = if normalized.is_empty() {
            "text"
        } else {
            normalized
        };
        self.current_lang = language.to_string();
        app_println!("{}", self.formatter.build_code_block_header(language));
        self.in_code_block = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // CodeFormatter Creation and Initialization Tests
    // ============================================================================

    #[test]
    fn test_code_formatter_creation() {
        let formatter = create_code_formatter();
        assert!(formatter.is_ok());
    }

    #[test]
    fn test_code_formatter_new() {
        let formatter = CodeFormatter::new();
        assert!(formatter.is_ok());
    }

    #[test]
    fn test_code_formatter_clone() {
        let formatter = create_code_formatter().unwrap();
        let cloned = formatter.clone();

        // Test that cloned formatter works correctly
        let input = "Test @file.txt";
        let result1 = formatter.format_input_with_file_highlighting(input);
        let result2 = cloned.format_input_with_file_highlighting(input);
        assert_eq!(result1, result2);
    }

    // ============================================================================
    // Language Normalization Tests
    // ============================================================================

    #[test]
    fn test_normalize_language_javascript() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("js"), "javascript");
        assert_eq!(formatter.normalize_language("jsx"), "javascript");
        assert_eq!(formatter.normalize_language("javascript"), "javascript");
    }

    #[test]
    fn test_normalize_language_typescript() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("ts"), "typescript");
        assert_eq!(formatter.normalize_language("tsx"), "typescript");
        assert_eq!(formatter.normalize_language("typescript"), "typescript");
    }

    #[test]
    fn test_normalize_language_python() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("py"), "python");
        assert_eq!(formatter.normalize_language("python"), "python");
    }

    #[test]
    fn test_normalize_language_rust() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("rs"), "rust");
        assert_eq!(formatter.normalize_language("rust"), "rust");
    }

    #[test]
    fn test_normalize_language_shell() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("sh"), "bash");
        assert_eq!(formatter.normalize_language("bash"), "bash");
        assert_eq!(formatter.normalize_language("zsh"), "bash");
    }

    #[test]
    fn test_normalize_language_yaml() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("yml"), "yaml");
        assert_eq!(formatter.normalize_language("yaml"), "yaml");
    }

    #[test]
    fn test_normalize_language_cpp() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("cpp"), "cpp");
        assert_eq!(formatter.normalize_language("cxx"), "cpp");
        assert_eq!(formatter.normalize_language("cc"), "cpp");
    }

    #[test]
    fn test_normalize_language_markdown() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("md"), "markdown");
        assert_eq!(formatter.normalize_language("markdown"), "markdown");
    }

    #[test]
    fn test_normalize_language_empty() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language(""), "text");
    }

    #[test]
    fn test_normalize_language_unknown() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("unknown"), "unknown");
    }

    #[test]
    fn test_normalize_language_case_insensitive() {
        let formatter = create_code_formatter().unwrap();
        // For known aliases, case doesn't matter
        assert_eq!(formatter.normalize_language("RS"), "rust");
        assert_eq!(formatter.normalize_language("rs"), "rust");
        assert_eq!(formatter.normalize_language("JS"), "javascript");
        assert_eq!(formatter.normalize_language("js"), "javascript");
    }

    // ============================================================================
    // File Highlighting Tests
    // ============================================================================

    #[test]
    fn test_file_highlighting_single_file() {
        let formatter = create_code_formatter().unwrap();
        let input = "Please read @test_file.txt";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@test_file.txt"));
    }

    #[test]
    fn test_file_highlighting_multiple_files() {
        let formatter = create_code_formatter().unwrap();
        let input = "Compare @file1.txt and @file2.txt";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@file1.txt"));
        assert!(highlighted.contains("@file2.txt"));
    }

    #[test]
    fn test_file_highlighting_no_files() {
        let formatter = create_code_formatter().unwrap();
        let input = "Just a regular message without files";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert_eq!(input, highlighted);
    }

    #[test]
    fn test_file_highlighting_with_path() {
        let formatter = create_code_formatter().unwrap();
        let input = "Check @src/main.rs for details";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@src/main.rs"));
    }

    #[test]
    fn test_file_highlighting_multiple_paths() {
        let formatter = create_code_formatter().unwrap();
        let input = "Compare @src/lib.rs and @tests/integration.rs";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@src/lib.rs"));
        assert!(highlighted.contains("@tests/integration.rs"));
    }

    #[test]
    fn test_file_highlighting_with_extensions() {
        let formatter = create_code_formatter().unwrap();
        let input = "Check @config.toml, @package.json, and @Cargo.lock";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@config.toml"));
        assert!(highlighted.contains("@package.json"));
        assert!(highlighted.contains("@Cargo.lock"));
    }

    #[test]
    fn test_file_highlighting_at_start() {
        let formatter = create_code_formatter().unwrap();
        let input = "@file.txt is the main config";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@file.txt"));
    }

    #[test]
    fn test_file_highlighting_at_end() {
        let formatter = create_code_formatter().unwrap();
        let input = "The config is in @file.txt";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@file.txt"));
    }

    #[test]
    fn test_file_highlighting_cache_hit() {
        let formatter = create_code_formatter().unwrap();
        let input = "Test @file.txt";

        // First call should populate cache
        let result1 = formatter.format_input_with_file_highlighting(input);

        // Second call should hit cache and return same result
        let result2 = formatter.format_input_with_file_highlighting(input);

        assert_eq!(result1, result2);
    }

    #[test]
    fn test_file_highlighting_cache_miss() {
        let formatter = create_code_formatter().unwrap();
        let input1 = "Test @file1.txt";
        let input2 = "Test @file2.txt";

        let result1 = formatter.format_input_with_file_highlighting(input1);
        let result2 = formatter.format_input_with_file_highlighting(input2);

        // Results should be different since inputs are different
        assert_ne!(result1, result2);
    }

    #[test]
    fn test_file_highlighting_fast_path_no_at_symbol() {
        let formatter = create_code_formatter().unwrap();
        let input = "This has no file references at all";
        let result = formatter.format_input_with_file_highlighting(input);
        assert_eq!(input, result);
    }

    // ============================================================================
    // Code Block Formatting Tests
    // ============================================================================

    #[test]
    fn test_format_response_no_code_blocks() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "Just plain text without any code blocks";
        let result = formatter.format_response(input)?;
        assert!(result.contains("plain text"));
        Ok(())
    }

    #[test]
    fn test_format_response_single_code_block() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "Here's some code:\n```rust\nfn main() {}\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("fn main"));
        Ok(())
    }

    #[test]
    fn test_format_response_multiple_code_blocks() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "First:\n```rust\nfn foo() {}\n```\nSecond:\n```python\ndef bar():\n    pass\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("fn foo"));
        assert!(result.contains("def bar"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_rust() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "fn main() {\n    let x = 42;\n}";
        let result = formatter.format_code_block(code, "rust")?;
        assert!(result.contains("fn main"));
        assert!(result.contains("RUST"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_python() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "def hello():\n    print('Hello')";
        let result = formatter.format_code_block(code, "python")?;
        assert!(result.contains("def hello"));
        assert!(result.contains("PYTHON"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_javascript() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "function test() {\n    return 42;\n}";
        let result = formatter.format_code_block(code, "javascript")?;
        assert!(result.contains("function test"));
        assert!(result.contains("JAVASCRIPT"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_typescript() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "interface User {\n    name: string;\n}";
        let result = formatter.format_code_block(code, "typescript")?;
        assert!(result.contains("interface User"));
        assert!(result.contains("TYPESCRIPT"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_json() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = r#"{"key": "value", "number": 42}"#;
        let result = formatter.format_code_block(code, "json")?;
        assert!(result.contains("key"));
        assert!(result.contains("JSON"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_yaml() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "name: test\nversion: 1.0";
        let result = formatter.format_code_block(code, "yaml")?;
        assert!(result.contains("name"));
        assert!(result.contains("YAML"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_bash() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "#!/bin/bash\necho 'Hello'";
        let result = formatter.format_code_block(code, "bash")?;
        assert!(result.contains("echo"));
        assert!(result.contains("BASH"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_sql() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "SELECT * FROM users WHERE id = 1";
        let result = formatter.format_code_block(code, "sql")?;
        assert!(result.contains("SELECT"));
        assert!(result.contains("SQL"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_empty() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "";
        let result = formatter.format_code_block(code, "rust")?;
        assert!(result.contains("RUST"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_unknown_language() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "some code here";
        let result = formatter.format_code_block(code, "unknown")?;
        assert!(result.contains("UNKNOWN"));
        assert!(result.contains("some code"));
        Ok(())
    }

    // ============================================================================
    // Language-Specific Highlighting Tests
    // ============================================================================

    #[test]
    fn test_highlight_rust_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "fn main() { let mut x = 42; }";
        let result = formatter.highlight_line(line, "rust");
        assert!(result.contains("main"));
    }

    #[test]
    fn test_highlight_rust_types() {
        let formatter = create_code_formatter().unwrap();
        let line = "let s: String = String::from(\"test\");";
        let result = formatter.highlight_line(line, "rust");
        assert!(result.contains("String"));
    }

    #[test]
    fn test_highlight_rust_comments() {
        let formatter = create_code_formatter().unwrap();
        let line = "// This is a comment";
        let result = formatter.highlight_line(line, "rust");
        assert!(result.contains("comment"));
    }

    #[test]
    fn test_highlight_python_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "def function(): pass";
        let result = formatter.highlight_line(line, "python");
        assert!(result.contains("function"));
    }

    #[test]
    fn test_highlight_python_comments() {
        let formatter = create_code_formatter().unwrap();
        let line = "# This is a comment";
        let result = formatter.highlight_line(line, "python");
        assert!(result.contains("comment"));
    }

    #[test]
    fn test_highlight_javascript_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "const x = function() { return 42; }";
        let result = formatter.highlight_line(line, "javascript");
        assert!(result.contains("return"));
    }

    #[test]
    fn test_highlight_typescript_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "interface User { name: string; }";
        let result = formatter.highlight_line(line, "typescript");
        assert!(result.contains("User"));
    }

    #[test]
    fn test_highlight_json_keys() {
        let formatter = create_code_formatter().unwrap();
        let line = r#"  "name": "value""#;
        let result = formatter.highlight_line(line, "json");
        assert!(result.contains("name"));
    }

    #[test]
    fn test_highlight_yaml_keys() {
        let formatter = create_code_formatter().unwrap();
        let line = "name: value";
        let result = formatter.highlight_line(line, "yaml");
        assert!(result.contains("name"));
    }

    #[test]
    fn test_highlight_html_tags() {
        let formatter = create_code_formatter().unwrap();
        let line = "<div class=\"test\">Content</div>";
        let result = formatter.highlight_line(line, "html");
        assert!(result.contains("div"));
    }

    #[test]
    fn test_highlight_css_properties() {
        let formatter = create_code_formatter().unwrap();
        let line = "color: red;";
        let result = formatter.highlight_line(line, "css");
        assert!(result.contains("color"));
    }

    #[test]
    fn test_highlight_bash_commands() {
        let formatter = create_code_formatter().unwrap();
        let line = "echo 'Hello World'";
        let result = formatter.highlight_line(line, "bash");
        assert!(result.contains("Hello"));
    }

    #[test]
    fn test_highlight_sql_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "SELECT * FROM users";
        let result = formatter.highlight_line(line, "sql");
        assert!(result.contains("users"));
    }

    #[test]
    fn test_highlight_markdown_headers() {
        let formatter = create_code_formatter().unwrap();
        let line = "# Header 1";
        let result = formatter.highlight_line(line, "markdown");
        assert!(result.contains("Header"));
    }

    #[test]
    fn test_highlight_markdown_bold() {
        let formatter = create_code_formatter().unwrap();
        let line = "This is **bold** text";
        let result = formatter.highlight_line(line, "markdown");
        assert!(result.contains("bold"));
    }

    #[test]
    fn test_highlight_toml_sections() {
        let formatter = create_code_formatter().unwrap();
        let line = "[package]";
        let result = formatter.highlight_line(line, "toml");
        assert!(result.contains("package"));
    }

    #[test]
    fn test_highlight_toml_keys() {
        let formatter = create_code_formatter().unwrap();
        let line = "name = \"test\"";
        let result = formatter.highlight_line(line, "toml");
        assert!(result.contains("name"));
    }

    #[test]
    fn test_highlight_c_cpp_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "int main() { return 0; }";
        let result = formatter.highlight_line(line, "c");
        assert!(result.contains("main"));
    }

    #[test]
    fn test_highlight_java_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "public class Main { }";
        let result = formatter.highlight_line(line, "java");
        assert!(result.contains("Main"));
    }

    #[test]
    fn test_highlight_go_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "func main() { }";
        let result = formatter.highlight_line(line, "go");
        assert!(result.contains("main"));
    }

    #[test]
    fn test_highlight_numbers() {
        let formatter = create_code_formatter().unwrap();
        let line = "The answer is 42 and pi is 3.14";
        let result = formatter.highlight_numbers(line);
        assert!(result.contains("42"));
        assert!(result.contains("3.14"));
    }

    #[test]
    fn test_highlight_numbers_various_formats() {
        let formatter = create_code_formatter().unwrap();
        let line = "Numbers: 0 1 123 456.789 0.5";
        let result = formatter.highlight_numbers(line);
        assert!(result.contains("0"));
        assert!(result.contains("123"));
        assert!(result.contains("456.789"));
    }

    // ============================================================================
    // Code Block Header/Footer Tests
    // ============================================================================

    #[test]
    fn test_build_code_block_header() {
        let formatter = create_code_formatter().unwrap();
        let header = formatter.build_code_block_header("rust");
        assert!(header.contains("RUST"));
        assert!(header.contains("┌"));
        assert!(header.contains("┐"));
    }

    #[test]
    fn test_build_code_block_footer() {
        let formatter = create_code_formatter().unwrap();
        let footer = formatter.build_code_block_footer("rust");
        assert!(footer.contains("└"));
        assert!(footer.contains("┘"));
        assert!(footer.contains("─"));
    }

    #[test]
    fn test_build_code_block_header_long_language() {
        let formatter = create_code_formatter().unwrap();
        let header = formatter.build_code_block_header("typescript");
        assert!(header.contains("TYPESCRIPT"));
    }

    #[test]
    fn test_build_code_block_footer_long_language() {
        let formatter = create_code_formatter().unwrap();
        let footer = formatter.build_code_block_footer("typescript");
        // Footer width should accommodate the language name
        assert!(footer.len() > 10);
    }

    // ============================================================================
    // StreamingResponseFormatter Tests
    // ============================================================================

    #[test]
    fn test_streaming_formatter_creation() {
        let formatter = create_code_formatter().unwrap();
        let _streaming = StreamingResponseFormatter::new(formatter);
    }

    #[test]
    fn test_streaming_formatter_handle_empty_chunk() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("")?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_handle_simple_text() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("Hello\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_handle_code_block() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("```rust\n")?;
        streaming.handle_chunk("fn main() {}\n")?;
        streaming.handle_chunk("```\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_multiple_chunks() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("First line\n")?;
        streaming.handle_chunk("Second line\n")?;
        streaming.handle_chunk("Third line\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_partial_line() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("Partial ")?;
        streaming.handle_chunk("line\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_finish_with_pending() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("No newline")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_code_block_with_language() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("```python\n")?;
        streaming.handle_chunk("def test():\n")?;
        streaming.handle_chunk("    pass\n")?;
        streaming.handle_chunk("```\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_mixed_content() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("Some text\n")?;
        streaming.handle_chunk("```rust\n")?;
        streaming.handle_chunk("fn foo() {}\n")?;
        streaming.handle_chunk("```\n")?;
        streaming.handle_chunk("More text\n")?;
        streaming.finish()?;
        Ok(())
    }

    // ============================================================================
    // Edge Cases and Special Scenarios
    // ============================================================================

    #[test]
    fn test_format_response_with_nested_backticks() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "Code: ```rust\nlet s = \"`test`\";\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("test"));
        Ok(())
    }

    #[test]
    fn test_format_response_with_special_characters() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "Special: ```\n<>&\"'\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("<>&"));
        Ok(())
    }

    #[test]
    fn test_file_highlighting_with_special_characters() {
        let formatter = create_code_formatter().unwrap();
        let input = "File: @test-file_123.txt";
        let result = formatter.format_input_with_file_highlighting(input);
        assert!(result.contains("@test-file_123.txt"));
    }

    #[test]
    fn test_file_highlighting_multiple_at_symbols() {
        let formatter = create_code_formatter().unwrap();
        let input = "Check @file1.txt @ @file2.txt";
        let result = formatter.format_input_with_file_highlighting(input);
        assert!(result.contains("@file1.txt"));
        assert!(result.contains("@file2.txt"));
    }

    #[test]
    fn test_empty_input() -> Result<()> {
        let formatter = create_code_formatter()?;
        let result = formatter.format_response("")?;
        assert_eq!(result, "");
        Ok(())
    }

    #[test]
    fn test_file_highlighting_empty_input() {
        let formatter = create_code_formatter().unwrap();
        let result = formatter.format_input_with_file_highlighting("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_whitespace_only_input() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "   \n\n   ";
        let result = formatter.format_response(input)?;
        assert!(result.contains("   "));
        Ok(())
    }

    #[test]
    fn test_code_block_with_empty_language() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "```\nplain code\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("plain code"));
        Ok(())
    }

    #[test]
    fn test_incomplete_code_block() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "```rust\nfn main() {}";
        let result = formatter.format_response(input)?;
        // Should handle gracefully - incomplete blocks are not formatted
        assert!(result.contains("```rust"));
        Ok(())
    }

    #[test]
    fn test_consecutive_code_blocks() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "```rust\nfn foo() {}\n```\n```python\ndef bar():\n    pass\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("fn foo"));
        assert!(result.contains("def bar"));
        Ok(())
    }

    #[test]
    fn test_highlight_line_unknown_language() {
        let formatter = create_code_formatter().unwrap();
        let line = "some random code 123";
        let result = formatter.highlight_line(line, "unknown");
        assert!(result.contains("code"));
        assert!(result.contains("123"));
    }

    #[test]
    fn test_normalize_language_mixed_case() {
        let formatter = create_code_formatter().unwrap();
        // For unknown full names, returns as-is (not normalized unless it's an alias)
        assert_eq!(formatter.normalize_language("RuSt"), "RuSt");
        assert_eq!(formatter.normalize_language("PyThOn"), "PyThOn");
        // But aliases work regardless of case
        assert_eq!(formatter.normalize_language("PY"), "python");
        assert_eq!(formatter.normalize_language("RS"), "rust");
    }

    #[test]
    fn test_text_before_and_after_code_blocks() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "Before\n```rust\ncode\n```\nAfter";
        let result = formatter.format_response(input)?;
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
        assert!(result.contains("code"));
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_empty_code_block() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("```rust\n")?;
        streaming.handle_chunk("```\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_code_block_without_language() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("```\n")?;
        streaming.handle_chunk("code here\n")?;
        streaming.handle_chunk("```\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_highlight_rust_string_with_escapes() {
        let formatter = create_code_formatter().unwrap();
        let line = r#"let s = "Hello \"World\"";"#;
        let result = formatter.highlight_line(line, "rust");
        assert!(result.contains("Hello"));
    }

    #[test]
    fn test_highlight_python_triple_quotes() {
        let formatter = create_code_formatter().unwrap();
        let line = r#""""docstring""""#;
        let result = formatter.highlight_line(line, "python");
        assert!(result.contains("docstring"));
    }

    #[test]
    fn test_highlight_javascript_template_strings() {
        let formatter = create_code_formatter().unwrap();
        let line = "const s = `template ${var}`;";
        let result = formatter.highlight_line(line, "javascript");
        assert!(result.contains("template"));
    }

    #[test]
    fn test_multiple_languages_in_single_response() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "Rust:\n```rust\nfn main() {}\n```\nPython:\n```python\ndef main():\n    pass\n```\nJS:\n```javascript\nfunction main() {}\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("RUST"));
        assert!(result.contains("PYTHON"));
        assert!(result.contains("JAVASCRIPT"));
        Ok(())
    }

    #[test]
    fn test_file_highlighting_with_dots_and_dashes() {
        let formatter = create_code_formatter().unwrap();
        let input = "Files: @my-file.test.ts and @another_file-v2.json";
        let result = formatter.format_input_with_file_highlighting(input);
        assert!(result.contains("@my-file.test.ts"));
        assert!(result.contains("@another_file-v2.json"));
    }

    #[test]
    fn test_cache_invalidation_on_different_input() {
        let formatter = create_code_formatter().unwrap();

        // Populate cache with first input
        let input1 = "Test @file1.txt";
        let _result1 = formatter.format_input_with_file_highlighting(input1);

        // Different input should not use cached result
        let input2 = "Test @file2.txt";
        let result2 = formatter.format_input_with_file_highlighting(input2);

        assert!(result2.contains("@file2.txt"));
    }

    #[test]
    fn test_format_text_with_file_highlighting_direct() {
        let formatter = create_code_formatter().unwrap();
        let input = "Check @readme.md and @license.txt";
        let result = formatter.format_text_with_file_highlighting(input);
        assert!(result.contains("@readme.md"));
        assert!(result.contains("@license.txt"));
    }

    #[test]
    fn test_code_block_footer_width_calculation() {
        let formatter = create_code_formatter().unwrap();
        let footer_short = formatter.build_code_block_footer("c");
        let footer_long = formatter.build_code_block_footer("typescript");

        // Longer language name should result in longer footer
        assert!(footer_long.len() > footer_short.len());
    }
}
