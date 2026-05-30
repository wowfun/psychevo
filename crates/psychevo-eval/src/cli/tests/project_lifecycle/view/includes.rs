#[allow(unused_imports)]
use super::*;

#[test]
pub(crate) fn view_include_all_expands_to_stable_diagnostic_set() {
    let includes = parse_view_includes(&["comparison,all".to_string(), "core".to_string()])
        .expect("all include parses");
    assert_eq!(includes, all_view_includes());

    let view = Cli::try_parse_from(["peval", "view", "-i", "all"]).expect("-i all parses");
    let Commands::View(args) = view.command else {
        panic!("expected view command");
    };
    assert_eq!(args.include, vec!["all".to_string()]);
}

#[test]
pub(crate) fn view_timeline_include_is_removed() {
    let err = parse_view_includes(&["timeline".to_string()]).expect_err("timeline fails");
    assert!(format!("{err:#}").contains("view include `timeline` is not supported"));
}

#[test]
pub(crate) fn view_atif_include_is_removed() {
    let err = parse_view_includes(&["atif".to_string()]).expect_err("atif fails");
    let message = format!("{err:#}");
    assert!(message.contains("view include `atif` is not supported"));
    assert!(message.contains("core"));
}

#[test]
pub(crate) fn view_logs_and_diff_includes_are_removed() {
    for include in ["logs", "diff"] {
        let err = parse_view_includes(&[include.to_string()]).expect_err("include fails");
        let message = format!("{err:#}");
        assert!(message.contains(&format!("view include `{include}` is not supported")));
        assert!(message.contains("attachments"));
    }
}
