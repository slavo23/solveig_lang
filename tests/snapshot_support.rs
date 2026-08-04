use std::path::Path;

pub fn assert_fixture_snapshot(snapshot_dir: &Path, label: &str, expected: &str, actual: &str) {
    pretty_assertions::assert_eq!(expected, actual, "fixture snapshot `{label}` changed");

    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(snapshot_dir);
    settings.set_prepend_module_to_snapshot(false);

    settings.bind(|| {
        insta::assert_snapshot!("expected", actual);
    });
}
