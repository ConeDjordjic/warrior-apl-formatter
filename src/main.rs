use dioxus::prelude::*;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::BTreeMap;

fn main() {
    launch(App);
}

static TOKEN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(<=|>=|<|>|=|&|\||\(|\)|[^<>=&|\(\)\s]+)").unwrap());

static NOT_TALENT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"!talent\.([a-zA-Z0-9_\.]+)").unwrap());
static TALENT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"talent\.([a-zA-Z0-9_\.]+)").unwrap());

fn tokenize_line(line: &str) -> Vec<&str> {
    TOKEN_RE.find_iter(line).map(|m| m.as_str()).collect()
}

#[derive(Debug)]
enum Expr {
    Atom(String),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

fn split_top_level<'a>(tokens: &'a [&'a str], op: &str) -> Option<Vec<&'a [&'a str]>> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut last = 0;
    for (i, &tok) in tokens.iter().enumerate() {
        if tok == "(" {
            depth += 1;
        } else if tok == ")" {
            depth -= 1;
        } else if depth == 0 && tok == op {
            parts.push(&tokens[last..i]);
            last = i + 1;
        }
    }
    if parts.is_empty() {
        None
    } else {
        parts.push(&tokens[last..]);
        Some(parts)
    }
}

fn parse_expr(tokens: &[&str]) -> Expr {
    if let Some(parts) = split_top_level(tokens, "or") {
        return Expr::Or(parts.into_iter().map(parse_expr).collect());
    }

    if let Some(parts) = split_top_level(tokens, "and") {
        return Expr::And(parts.into_iter().map(parse_expr).collect());
    }

    if tokens.len() >= 2 && tokens[0] == "(" && tokens[tokens.len() - 1] == ")" {
        let inner_tokens = &tokens[1..tokens.len() - 1];
        let mut depth = 0;
        let mut is_single_group = true;
        for &tok in inner_tokens.iter() {
            if tok == "(" {
                depth += 1;
            } else if tok == ")" {
                depth -= 1;
            }
            if depth < 0 {
                is_single_group = false;
                break;
            }
        }

        if is_single_group && depth == 0 {
            return parse_expr(inner_tokens);
        }
    }

    Expr::Atom(tokens.join(" "))
}

fn pretty_format_condition(expr: &Expr, indent: usize) -> String {
    let indent_str = "    ".repeat(indent);

    match expr {
        Expr::Atom(s) => format!("{}{}", indent_str, s),
        Expr::And(parts) => {
            if parts.is_empty() {
                return "".to_string();
            }
            if parts.len() == 1 {
                return pretty_format_condition(&parts[0], indent);
            }

            let formatted_parts: Vec<String> = parts
                .iter()
                .map(|part| {
                    let is_multiline_or = if let Expr::Or(sub_parts) = part {
                        sub_parts.len() > 1
                    } else {
                        false
                    };

                    if is_multiline_or {
                        let or_content = pretty_format_condition(part, indent + 1);
                        format!("(\n{}\n{})", or_content, indent_str)
                    } else {
                        let formatted = pretty_format_condition(part, 0);
                        format!("{}", formatted.trim())
                    }
                })
                .collect();

            format!("{}{}", indent_str, formatted_parts.join(" AND "))
        }
        Expr::Or(parts) => {
            if parts.is_empty() {
                return "".to_string();
            }
            if parts.len() == 1 {
                return pretty_format_condition(&parts[0], indent);
            }

            let formatted_parts: Vec<String> = parts
                .iter()
                .enumerate()
                .map(|(i, part)| {
                    if i == 0 {
                        pretty_format_condition(part, indent)
                    } else {
                        let formatted = pretty_format_condition(part, 0);
                        format!("{}OR {}", indent_str, formatted.trim())
                    }
                })
                .collect();

            formatted_parts.join("\n")
        }
    }
}

fn transform_condition(raw: &str) -> String {
    let result = NOT_TALENT_RE.replace_all(raw, "$1 not talented");
    let result = TALENT_RE.replace_all(&result, "$1 talented");

    let tokens = tokenize_line(&result);
    let transformed_tokens: Vec<String> = tokens
        .iter()
        .map(|token| match *token {
            "&" => "and".to_string(),
            "|" => "or".to_string(),
            "!" => "not".to_string(),
            other => other.replace("debuff.", "").replace("buff.", ""),
        })
        .collect();

    let expr = parse_expr(
        &transformed_tokens
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>(),
    );

    pretty_format_condition(&expr, 1)
}

fn process_line(line: &str) -> Option<(String, String)> {
    let (when_raw, spell_raw) = if let Some((a, b)) = line.split_once("+=/") {
        (a.trim(), b.trim())
    } else if let Some((a, b)) = line.split_once('=') {
        (a.trim(), b.trim())
    } else {
        return None;
    };

    let when = when_raw
        .strip_prefix("actions.")
        .unwrap_or(when_raw)
        .to_string();

    let (spell, condition_opt) = if let Some((s, cond)) = spell_raw.split_once(",if=") {
        (s.trim(), Some(cond.trim()))
    } else {
        (spell_raw.trim(), None)
    };

    let result = if let Some(cond_str) = condition_opt {
        let formatted_condition = transform_condition(cond_str);
        format!("{}:\n{}", spell, formatted_condition)
    } else {
        spell.to_string()
    };

    Some((when, result))
}

fn process_apl_grouped(apl: &str) -> BTreeMap<String, Vec<String>> {
    let mut groups = BTreeMap::new();

    for line in apl.lines() {
        let trimmed_line = line.trim();
        if trimmed_line.is_empty() || trimmed_line.starts_with('#') {
            continue;
        }

        if let Some((when, spell_and_condition)) = process_line(trimmed_line) {
            groups
                .entry(when)
                .or_insert_with(Vec::new)
                .push(spell_and_condition);
        }
    }
    groups
}

#[component]
fn App() -> Element {
    let mut input = use_signal(|| "".to_string());
    let groups = process_apl_grouped(&input());

    rsx! {
        head {
            style {
                {r#"
                html, body {
                    margin: 0;
                    padding: 0;
                    background-color: #05080d;
                    color: #f3f4f6;
                    height: 100%;
                    font-family: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", "Roboto", sans-serif;
                }

                * {
                    box-sizing: border-box;
                }

                .app-container {
                    display: flex;
                    flex-direction: column;
                    gap: 1.5rem;
                    max-width: 1200px;
                    padding: 1.5rem;
                    margin: 0 auto;
                    background-color: #05080d;
                    color: #f3f4f6;
                    min-height: 100vh;
                }

                .main-input {
                    width: 100%;
                    font-family: "SF Mono", "Monaco", "Cascadia Code", "Roboto Mono", Consolas, "Courier New", monospace;
                    background-color: #1f2937;
                    color: #f3f4f6;
                    border: 1px solid #4b5563;
                    padding: 0.75rem;
                    border-radius: 0.375rem;
                    outline: none;
                    transition: all 0.2s ease-in-out;
                    resize: vertical;
                    min-height: 200px;
                    font-size: 14px;
                    line-height: 1.5;
                }

                .main-input:focus {
                    border-color: #3b82f6;
                    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.1);
                }

                .main-input::placeholder {
                    color: #9ca3af;
                }

                .groups-grid {
                    display: grid;
                    gap: 1.5rem;
                    width: 100%;
                    grid-template-columns: repeat(auto-fit, minmax(500px, 1fr));
                    grid-auto-rows: 1fr;
                }

                .group-card {
                    display: flex;
                    flex-direction: column;
                    border-radius: 0.5rem;
                    box-shadow: 0 4px 8px rgba(0, 0, 0, 0.3);
                    overflow: hidden;
                    background-color: #1f2937;
                    border: 1px solid #4b5563;
                    min-height: 400px;
                }

                .group-header {
                    margin: 0;
                    padding: 0.75rem 1rem;
                    background-color: #374151;
                    color: #f3f4f6;
                    font-family: "SF Mono", "Monaco", "Cascadia Code", "Roboto Mono", Consolas, "Courier New", monospace;
                    font-size: 0.875rem;
                    font-weight: 600;
                    text-align: center;
                    border-bottom: 1px solid #4b5563;
                    flex-shrink: 0;
                }

                .group-content {
                    width: 100%;
                    flex: 1;
                    font-family: "SF Mono", "Monaco", "Cascadia Code", "Roboto Mono", Consolas, "Courier New", monospace;
                    background-color: #1f2937;
                    color: #f3f4f6;
                    border: none;
                    padding: 0.75rem;
                    outline: none;
                    resize: none;
                    overflow-y: auto;
                    line-height: 1.6;
                    white-space: pre-wrap;
                    word-wrap: break-word;
                    font-size: 13px;
                    tab-size: 4;
                }

                .group-content:focus {
                    background-color: #1f2937;
                    box-shadow: inset 0 0 0 2px #3b82f6;
                }

                @media (max-width: 768px) {
                    .groups-grid {
                        grid-template-columns: 1fr;
                    }
                    .app-container {
                        padding: 1rem;
                    }
                }

                @media (min-width: 1400px) {
                    .app-container {
                        max-width: 1600px;
                    }
                    .groups-grid {
                        grid-template-columns: repeat(auto-fit, minmax(600px, 1fr));
                    }
                }
                "#}
            }
        }

        div {
            class: "app-container",

            textarea {
                rows: "15",
                class: "main-input",
                placeholder: "Paste your APL here...",
                value: "{input()}",
                oninput: move |e| input.set(e.value()),
            }

            div {
                class: "groups-grid",
                for (when_type, spells) in groups.iter() {
                    {
                        let numbered_spells = spells.iter()
                            .enumerate()
                            .map(|(i, spell)| format!("({}) {}", i + 1, spell))
                            .collect::<Vec<_>>()
                            .join("\n\n");

                        rsx! {
                            div {
                                key: "{when_type}",
                                class: "group-card",
                                h3 {
                                    class: "group-header",
                                    "{when_type}"
                                }
                                textarea {
                                    rows: "30",
                                    class: "group-content",
                                    readonly: true,
                                    value: "{numbered_spells}",
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
