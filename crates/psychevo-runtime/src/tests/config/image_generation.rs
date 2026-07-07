#[allow(unused_imports)]
pub(crate) use super::*;

#[test]
fn image_generation_config_parses_documented_block() {
    let config = crate::config::config_parse::parse_run_config(json!({
        "image_generation": {
            "provider": "fake",
            "model": "fake-image",
            "size": "1024x1536",
            "format": "webp"
        }
    }))
    .expect("image generation config");

    assert_eq!(config.image_generation.provider, "fake");
    assert_eq!(config.image_generation.model, "fake-image");
    assert_eq!(config.image_generation.size, "1024x1536");
    assert_eq!(
        config.image_generation.format,
        psychevo_ai::ImageGenerationFormat::Webp
    );
}

#[test]
fn image_generation_config_rejects_raw_keys_and_invalid_fields() {
    let raw_key = crate::config::config_parse::parse_run_config(json!({
        "image_generation": {
            "api_key": "secret"
        }
    }))
    .expect_err("raw key");
    assert!(
        raw_key
            .to_string()
            .contains("must not contain raw API keys")
    );

    let invalid_size = crate::config::config_parse::parse_run_config(json!({
        "image_generation": {
            "size": "2048x2048"
        }
    }))
    .expect_err("invalid size");
    assert!(invalid_size.to_string().contains("image_generation.size"));

    let invalid_format = crate::config::config_parse::parse_run_config(json!({
        "image_generation": {
            "format": "bmp"
        }
    }))
    .expect_err("invalid format");
    assert!(
        invalid_format
            .to_string()
            .contains("image_generation.format")
    );
}

#[test]
fn fake_image_generation_provider_resolves_without_credentials() {
    let temp = tempdir().expect("temp");
    fs::create_dir_all(home_dir(&temp)).expect("home");
    fs::write(
        home_dir(&temp).join("config.toml"),
        r#"
[image_generation]
provider = "fake"
model = "fake-image"
size = "auto"
format = "png"
"#,
    )
    .expect("config");
    let options = base_options(&temp);

    let resolved = crate::config::resolve_image_generation_config(&options, None, None, None, None)
        .expect("image generation");
    assert_eq!(resolved.provider, "fake");
    assert_eq!(resolved.model, "fake-image");
    assert_eq!(resolved.api_key, None);

    let value = crate::config::image_generation_config_value(&options).expect("config value");
    assert_eq!(value["provider"], "fake");
    assert_eq!(value["credentialStatus"], "notRequired");
}
