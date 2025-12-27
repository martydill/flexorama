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
            let highlighted_file = format!("@{}", file_path.on_bright_blue().white().bold());
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


