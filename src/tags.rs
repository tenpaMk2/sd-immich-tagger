use regex::Regex;
use std::sync::LazyLock;

static LORA_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<lora:([^>:]+):\d+\.?\d*>").expect("valid lora regex"));
static LORA_REMOVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<lora:[^>]+:\d+\.?\d*>").expect("valid lora remove regex"));
static WHITESPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("valid whitespace regex"));

/// Extract tags from Stable Diffusion generation info, matching immich_uploader.py logic.
pub fn extract_tags_from_info(generation_info: &str) -> Vec<String> {
    if generation_info.is_empty() {
        return Vec::new();
    }

    let prompt_part = generation_info
        .split("Negative prompt:")
        .next()
        .unwrap_or(generation_info);

    let mut tags = Vec::new();

    for cap in LORA_PATTERN.captures_iter(prompt_part) {
        let lora = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if lora.is_empty() {
            continue;
        }
        let lora_tag = format!("<lora:{lora}>");
        if !tags.contains(&lora_tag) {
            tags.push(lora_tag);
        }
    }

    let mut prompt_part = LORA_REMOVE.replace_all(prompt_part, "").into_owned();

    let (weighted_tags, without_weights) = extract_weighted_tags(&prompt_part);
    for weight in weighted_tags {
        if !tags.contains(&weight) {
            tags.push(weight);
        }
    }
    prompt_part = without_weights;

    for part in prompt_part.split(',') {
        let mut cleaned = part.trim().to_string();
        cleaned = WHITESPACE.replace_all(&cleaned, " ").into_owned();
        if !cleaned.is_empty() && !tags.contains(&cleaned) {
            tags.push(cleaned);
        }
    }

    tags
}

fn extract_weighted_tags(input: &str) -> (Vec<String>, String) {
    let mut tags = Vec::new();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;

    while index < input.len() {
        let ch = input[index..].chars().next().expect("valid utf-8");
        if ch == '(' && !is_escaped(input, index) {
            if let Some((tag, consumed)) = parse_weighted_group(&input[index..]) {
                if !tag.is_empty() {
                    tags.push(tag);
                }
                index += consumed;
                continue;
            }
        }

        output.push(ch);
        index += ch.len_utf8();
    }

    (tags, output)
}

fn is_escaped(input: &str, index: usize) -> bool {
    let mut slash_count = 0;
    for ch in input[..index].chars().rev() {
        if ch == '\\' {
            slash_count += 1;
        } else {
            break;
        }
    }
    slash_count % 2 == 1
}

fn parse_weighted_group(input: &str) -> Option<(String, usize)> {
    if !input.starts_with('(') {
        return None;
    }

    let mut content = String::new();
    let mut index = 1;

    while index < input.len() {
        let ch = input[index..].chars().next()?;
        if ch == ')' && !is_escaped(input, index) {
            if let Some((tag, weight)) = content.rsplit_once(':') {
                if tag_weight_is_valid(tag, weight) {
                    return Some((tag.trim().to_string(), index + ch.len_utf8()));
                }
            }
            return None;
        }

        content.push(ch);
        index += ch.len_utf8();
    }

    None
}

fn tag_weight_is_valid(tag: &str, weight: &str) -> bool {
    !tag.is_empty() && weight.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
}

/// Truncate tag names longer than Immich's limit, matching get_or_create_tags.
pub fn truncate_tag_name(name: &str) -> String {
    if name.len() <= 100 {
        return name.to_string();
    }
    format!("{}...", &name[..97])
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn extracts_lora_weight_and_comma_tags() {
        let info = "masterpiece, <lora:style:0.8>, (detailed eyes:1.2), Negative prompt: bad";
        let tags = extract_tags_from_info(info);
        assert_eq!(
            tags,
            vec![
                "<lora:style>".to_string(),
                "detailed eyes".to_string(),
                "masterpiece".to_string(),
            ]
        );
    }

    #[test]
    fn ignores_escaped_parentheses() {
        let info = r"\(escaped:1.2), real, (weighted:1.0)";
        let tags = extract_tags_from_info(info);
        assert_eq!(
            tags,
            vec![
                "weighted".to_string(),
                r"\(escaped:1.2)".to_string(),
                "real".to_string(),
            ]
        );
    }

    #[test]
    fn returns_empty_for_blank_input() {
        assert!(extract_tags_from_info("").is_empty());
    }

    #[test]
    fn truncates_long_tag_names() {
        let long = "a".repeat(120);
        let truncated = truncate_tag_name(&long);
        assert_eq!(truncated.len(), 100);
        assert!(truncated.ends_with("..."));
    }
}
