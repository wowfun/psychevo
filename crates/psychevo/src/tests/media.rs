#[allow(unused_imports)]
pub(crate) use super::*;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};

fn one_pixel_png() -> Vec<u8> {
    BASE64_STANDARD
        .decode(psychevo_ai::DEFAULT_FAKE_IMAGE_BASE64)
        .expect("png fixture")
}

#[tokio::test]
async fn image_resolver_accepts_local_data_and_media_refs() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    let home = home_dir(&temp);
    fs::create_dir_all(&cwd).expect("cwd");
    fs::create_dir_all(&home).expect("home");
    let bytes = one_pixel_png();
    fs::write(cwd.join("pixel.png"), &bytes).expect("image");

    let local = crate::media::resolve_explicit_image_source("pixel.png", &cwd, &home)
        .await
        .expect("local image");
    assert_eq!(local.mime_type, "image/png");
    assert_eq!(local.size_bytes, bytes.len() as u64);

    let data = crate::media::resolve_explicit_image_source(
        &format!("data:image/png;base64,{}", BASE64_STANDARD.encode(&bytes)),
        &cwd,
        &home,
    )
    .await
    .expect("data image");
    assert_eq!(data.mime_type, "image/png");
    assert_eq!(data.display_source, "data:image");

    let artifact =
        crate::media::write_generated_image_artifact(&home, &bytes, "image/png").expect("artifact");
    let media =
        crate::media::resolve_explicit_image_source(&artifact.agent_visible_source, &cwd, &home)
            .await
            .expect("media ref");
    assert_eq!(media.display_source, artifact.display_url);
    assert_eq!(media.agent_visible_source, artifact.agent_visible_source);
}

#[tokio::test]
async fn image_resolver_rejects_mime_mismatch_and_unsafe_remote_hosts() {
    let temp = tempdir().expect("temp");
    let cwd = temp.path().join("work");
    let home = home_dir(&temp);
    fs::create_dir_all(&cwd).expect("cwd");
    fs::create_dir_all(&home).expect("home");
    let bytes = one_pixel_png();

    let mismatch = crate::media::resolve_explicit_image_source(
        &format!("data:image/jpeg;base64,{}", BASE64_STANDARD.encode(&bytes)),
        &cwd,
        &home,
    )
    .await
    .expect_err("mime mismatch");
    assert!(
        mismatch
            .to_string()
            .to_ascii_lowercase()
            .contains("mismatch"),
        "{mismatch}"
    );

    let unsafe_remote =
        crate::media::resolve_explicit_image_source("http://127.0.0.1/image.png", &cwd, &home)
            .await
            .expect_err("unsafe remote");
    assert!(
        unsafe_remote
            .to_string()
            .contains("remote image URL host is not allowed")
    );
}
