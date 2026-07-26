use serde_json::{Value, json};

const WEB_SEARCH_RESULT_PREFIX: &str = "<external_untrusted_web_search>\n";
const WEB_SEARCH_RESULT_SUFFIX: &str = "\n</external_untrusted_web_search>";

/// Decodes the persisted tool-result representation for display projections.
///
/// Local web search results are deliberately wrapped before they enter provider
/// context. The wrapper remains part of the durable message, while projections
/// recover the structured envelope for the ordinary tool-result UI.
pub fn decode_persisted_tool_result_for_display(tool_name: &str, content: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(content) {
        return value;
    }
    if tool_name == "web_search"
        && let Some(inner) = content
            .strip_prefix(WEB_SEARCH_RESULT_PREFIX)
            .and_then(|content| content.strip_suffix(WEB_SEARCH_RESULT_SUFFIX))
        && let Ok(value @ Value::Object(_)) = serde_json::from_str::<Value>(inner)
        && value.get("payload").is_some()
    {
        return value;
    }
    json!({ "content": content })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_only_exact_structured_web_search_wrappers() {
        let wrapped = concat!(
            "<external_untrusted_web_search>\n",
            r#"{"provider":"local","payload":{"items":[{"title":"Result"}]}}"#,
            "\n</external_untrusted_web_search>"
        );
        assert_eq!(
            decode_persisted_tool_result_for_display("web_search", wrapped)["payload"]["items"][0]
                ["title"],
            "Result"
        );

        for (tool_name, content) in [
            ("other_tool", wrapped),
            (
                "web_search",
                "prefix<external_untrusted_web_search>\n{}\n</external_untrusted_web_search>",
            ),
            (
                "web_search",
                "<external_untrusted_web_search>\nnot json\n</external_untrusted_web_search>",
            ),
            (
                "web_search",
                "<external_untrusted_web_search>\n{}\n</external_untrusted_web_search>",
            ),
        ] {
            assert_eq!(
                decode_persisted_tool_result_for_display(tool_name, content),
                json!({ "content": content })
            );
        }
    }

    #[test]
    fn preserves_ordinary_json_tool_results() {
        assert_eq!(
            decode_persisted_tool_result_for_display("exec_command", r#"{"exit_code":0}"#),
            json!({ "exit_code": 0 })
        );
    }
}
