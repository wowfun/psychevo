#![cfg(windows)]

use psychevo::application::WorkspaceRootCapture;

#[test]
fn captured_workspace_root_rejects_a_replacement_at_the_same_path() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("root");
    let original = temp.path().join("original");
    std::fs::create_dir(&root).expect("root");
    let captured =
        WorkspaceRootCapture::capture(std::slice::from_ref(&root)).expect("capture root");

    captured.validate().expect("unchanged root remains valid");
    std::fs::rename(&root, &original).expect("retain original object");
    std::fs::create_dir(&root).expect("replacement");

    let error = captured
        .validate()
        .expect_err("replacement must fail closed");

    assert!(error.to_string().contains("path_identity_changed"));
}

#[test]
fn captured_workspace_root_classifies_missing_and_non_directory_replacements() {
    for replacement_is_file in [false, true] {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("root");
        let original = temp.path().join("original");
        std::fs::create_dir(&root).expect("root");
        let captured =
            WorkspaceRootCapture::capture(std::slice::from_ref(&root)).expect("capture root");
        std::fs::rename(&root, &original).expect("retain original object");
        if replacement_is_file {
            std::fs::write(&root, b"replacement").expect("file replacement");
        }

        let error = captured
            .validate()
            .expect_err("missing or non-directory replacement must fail closed");
        assert!(
            error.to_string().contains("path_identity_changed"),
            "unexpected error: {error}"
        );
    }
}
