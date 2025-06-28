use dioxus::{document::Stylesheet, prelude::*};
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
    let indent_str = " ".repeat(indent * 4);
    match expr {
        Expr::Atom(s) => format!("{}{}", indent_str, s),
        Expr::And(parts) => {
            let inner = parts
                .iter()
                .map(|p| pretty_format_condition(p, 0).trim().to_string())
                .collect::<Vec<_>>()
                .join(" and ");
            format!("{}{}", indent_str, inner)
        }
        Expr::Or(parts) => parts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let prefix = if i == 0 { "" } else { "OR " };
                let formatted_part = pretty_format_condition(p, 0);

                formatted_part
                    .lines()
                    .enumerate()
                    .map(|(line_idx, line)| {
                        if line_idx == 0 {
                            format!("{}{}{}", indent_str, prefix, line.trim_start())
                        } else {
                            format!("{}{}", indent_str, line.trim_start())
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n"),
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
        Stylesheet {href: asset!("./assets/main.css")}

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
                                    rows: "{40}",
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
