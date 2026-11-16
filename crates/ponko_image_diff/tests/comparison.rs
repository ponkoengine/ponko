use ponko_image_diff::{Image, Tolerance, compare, heatmap};
const EXACT: Tolerance = Tolerance {
    channel: 0,
    outliers: 0.0,
    edge: 0,
};
fn view(rgba: &[u8]) -> Image<'_> {
    Image {
        width: (rgba.len() / 4) as u32,
        height: 1,
        rgba,
    }
}

#[test]
fn identical_images_have_perfect_score_and_no_regions() {
    let a = view(&[30, 50, 90, 255, 2, 6, 9, 255]);
    let d = compare(&a, &a, EXACT).unwrap();
    assert_eq!((d.max_channel, d.mean, d.outliers), (0, 0.0, 0));
    assert_eq!(d.ssim, 1.0);
    assert!(d.regions.is_empty());
}
#[test]
fn channel_and_fraction_boundaries_preserve_render_test_decisions() {
    let a = view(&[0; 16]);
    let b = view(&[4, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let tolerance = Tolerance {
        channel: 4,
        outliers: 0.25,
        edge: 0,
    };
    let d = compare(&a, &b, tolerance).unwrap();
    assert_eq!((d.outliers, d.max_channel, d.beyond), (1, 5, 5));
    assert_eq!(d.mean, 9.0 / 16.0);
    assert_eq!(d.percent_over, 25.0);
    assert!(d.within(tolerance));
    assert!(!d.within(Tolerance {
        outliers: 0.249,
        ..tolerance
    }));
    assert_eq!((d.regions[0].x, d.regions[0].width, d.regions[0].pixels), (1, 1, 1));
    assert_eq!(&heatmap(&a, &b, tolerance).unwrap()[4..8], &[255, 0, 0, 255]);
}
#[test]
fn edge_allowance_is_symmetric_and_missing_lines_still_fail() {
    let a = view(&[0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255]);
    let b = view(&[0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255]);
    let near = Tolerance { edge: 1, ..EXACT };
    assert_eq!(compare(&a, &b, EXACT).unwrap().outliers, 1);
    assert_eq!(compare(&a, &b, near).unwrap().outliers, 0);
    assert_eq!(compare(&b, &a, near).unwrap().outliers, 0);
    let flat = view(&[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
    assert!(compare(&a, &flat, near).unwrap().outliers > 0);
}
#[test]
fn size_mismatch_and_invalid_buffers_are_clear_errors() {
    let a = view(&[0; 4]);
    let b = view(&[0; 8]);
    assert!(compare(&a, &b, EXACT).unwrap_err().message().contains("size mismatch"));
    assert!(compare(&Image { rgba: &[], ..a }, &a, EXACT).is_err());
    assert!(
        compare(
            &a,
            &a,
            Tolerance {
                outliers: f32::NAN,
                ..EXACT
            }
        )
        .is_err()
    );
}
#[test]
fn alpha_errors_count_even_when_luminance_is_identical() {
    let a = view(&[0, 0, 0, 255]);
    let b = view(&[0, 0, 0, 0]);
    let d = compare(&a, &b, EXACT).unwrap();
    assert_eq!((d.outliers, d.max_channel, d.ssim), (1, 255, 1.0));
}
#[test]
fn regions_are_sorted_by_area_and_capped_at_eight() {
    let a = vec![0; 40 * 4];
    let mut b = a.clone();
    for x in (0..38).step_by(3) {
        b[x * 4] = 255;
    }
    b[4] = 255;
    let d = compare(&view(&a), &view(&b), EXACT).unwrap();
    assert_eq!(d.regions.len(), 8);
    assert_eq!((d.regions[0].x, d.regions[0].pixels), (0, 2));
    assert!(d.ssim < 1.0);
}

#[test]
fn file_errors_keep_the_path_kind_and_source() {
    use ponko_image_diff::{ErrorKind, compare_files};
    use std::error::Error as _;
    let path = std::path::Path::new("no-such-image-diff-input.png");
    let error = compare_files(path, path, EXACT, None).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Io);
    assert_eq!(error.path(), Some(path));
    assert!(error.source().is_some());
}

#[test]
fn invalid_comparison_inputs_have_a_matchable_kind() {
    let a = view(&[0; 4]);
    let error = compare(&a, &view(&[0; 8]), EXACT).unwrap_err();
    assert_eq!(error.kind(), ponko_image_diff::ErrorKind::Invalid);
    assert!(error.path().is_none());
}

#[test]
fn corrupt_images_and_existing_heatmaps_keep_actionable_causes() {
    use ponko_image_diff::{ErrorKind, compare_files};
    use std::error::Error as _;
    let root = std::env::temp_dir().join(format!("ponko-image-diff-errors-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let bad = root.join("broken.png");
    std::fs::write(&bad, b"not PNG bytes").unwrap();
    let error = compare_files(&bad, &bad, EXACT, None).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Decode);
    assert_eq!(error.path(), Some(bad.as_path()));
    assert!(error.source().is_some());
    assert!(
        error
            .message()
            .ends_with("check that the path names a readable PNG image"),
        "{error}"
    );
    let input = root.join("input.png");
    image::save_buffer(&input, &[0, 0, 0, 255], 1, 1, image::ColorType::Rgba8).unwrap();
    let output = root.join("existing.png");
    std::fs::write(&output, b"keep me").unwrap();
    let error = compare_files(&input, &input, EXACT, Some(&output)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Io);
    assert_eq!(error.path(), Some(output.as_path()));
    assert!(error.source().is_some());
    assert!(
        error
            .message()
            .ends_with("heatmaps never overwrite a file; choose a new output path"),
        "{error}"
    );
    assert!(
        error.to_string().starts_with(&format!("ponko: {}: ", output.display())),
        "{error}"
    );
    assert_eq!(std::fs::read(output).unwrap(), b"keep me");
    std::fs::remove_dir_all(root).unwrap();
}
