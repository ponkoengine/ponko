use std::path::PathBuf;
use std::process::Command;
#[test]
fn image_diff_prints_json_and_exits_one_only_beyond_threshold_or_on_error() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/render");
    let png = std::fs::read_dir(fixtures)
        .unwrap()
        .map(|p| p.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "png"))
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ponko"))
        .arg("image-diff")
        .arg(&png)
        .arg(&png)
        .output()
        .unwrap();
    assert!(output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["within"], true);
    let different = std::env::temp_dir().join(format!("ponko-cli-diff-{}.png", std::process::id()));
    ponko_image_diff::compare_files(
        &png,
        &png,
        ponko_image_diff::Tolerance {
            channel: 0,
            outliers: 0.0,
            edge: 0,
        },
        Some(&different),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ponko"))
        .arg("image-diff")
        .arg(&png)
        .arg(&different)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["within"], false);
    let output = Command::new(env!("CARGO_BIN_EXE_ponko"))
        .arg("image-diff")
        .arg(&png)
        .arg(&different)
        .args(["--threshold", "100"])
        .output()
        .unwrap();
    assert!(output.status.success());
    std::fs::remove_file(different).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ponko"))
        .args(["image-diff", "missing.png", "missing.png"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}
