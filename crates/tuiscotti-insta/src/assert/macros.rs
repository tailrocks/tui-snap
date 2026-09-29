//! Facade macros plus the caller [`Location`](super::Location) they capture.

/// Caller location captured by the facade macros.
#[derive(Debug, Clone, Copy)]
pub struct Location {
    /// `file!()` at the macro call site.
    pub file: &'static str,
    /// `line!()` at the macro call site.
    pub line: u32,
}

/// Assert styled canonical state through native Insta review (I01).
///
/// `$name` is the snapshot name (`&str` or `String`), `$screen` a `&Screen`.
/// An optional `&`[`Policy`](super::Policy) switches between the evolving review flow and a
/// frozen root. The Insta assertion expands AT THE CALLER: snapshot metadata
/// (file/module/test identity) names the call site. Each argument is
/// evaluated exactly once.
#[macro_export]
macro_rules! assert_snapshot {
    ($name:expr, $screen:expr) => {
        $crate::assert_snapshot!($name, $screen, &$crate::assert::Policy::Evolving)
    };
    ($name:expr, $screen:expr, $policy:expr) => {{
        let __tuiscotti_name = $name;
        let __tuiscotti_name: &str = __tuiscotti_name.as_ref();
        let __tuiscotti_screen = $screen;
        let __tuiscotti_policy: &$crate::assert::Policy = $policy;
        let __tuiscotti_location = $crate::assert::Location {
            file: file!(),
            line: line!(),
        };
        match __tuiscotti_policy {
            $crate::assert::Policy::Evolving => {
                let __tuiscotti_dir = $crate::assert::default_snapshot_dir();
                let (__tuiscotti_canonical, __tuiscotti_generation) =
                    $crate::assert::prepare_snapshot(__tuiscotti_screen);
                let __tuiscotti_settings = $crate::assert::snapshot_settings(
                    &__tuiscotti_dir,
                    __tuiscotti_location,
                    &__tuiscotti_generation,
                );
                let __tuiscotti_snap_name = __tuiscotti_name.to_string();
                __tuiscotti_settings.bind(|| {
                    $crate::insta::assert_snapshot!(
                        __tuiscotti_snap_name,
                        __tuiscotti_canonical,
                        "canonical screen"
                    );
                });
            }
            $crate::assert::Policy::EvolvingIn { snapshots, .. } => {
                let (__tuiscotti_canonical, __tuiscotti_generation) =
                    $crate::assert::prepare_snapshot(__tuiscotti_screen);
                let __tuiscotti_settings = $crate::assert::snapshot_settings(
                    snapshots,
                    __tuiscotti_location,
                    &__tuiscotti_generation,
                );
                let __tuiscotti_snap_name = __tuiscotti_name.to_string();
                __tuiscotti_settings.bind(|| {
                    $crate::insta::assert_snapshot!(
                        __tuiscotti_snap_name,
                        __tuiscotti_canonical,
                        "canonical screen"
                    );
                });
            }
            $crate::assert::Policy::Frozen { root } => {
                $crate::assert::assert_frozen_snapshot(root, __tuiscotti_name, __tuiscotti_screen);
            }
        }
    }};
}

/// Assert canonical state plus an independently rendered PNG as one sample (I02).
///
/// `$name` is the snapshot base (`&str` or `String`), `$screen` a `&Screen`.
/// Candidate evidence (`<name>.{png,ansi,txt,html}` under [`EVIDENCE_DIR_ENV`](super::EVIDENCE_DIR_ENV))
/// is written BEFORE any failure. The PNG snapshot is named `<name>-img`.
/// Both Insta assertions expand AT THE CALLER. Each argument is evaluated
/// exactly once.
#[macro_export]
macro_rules! assert_screenshot {
    ($name:expr, $screen:expr) => {
        $crate::assert_screenshot!($name, $screen, &$crate::assert::Policy::Evolving)
    };
    ($name:expr, $screen:expr, $policy:expr) => {{
        let __tuiscotti_name = $name;
        let __tuiscotti_name: &str = __tuiscotti_name.as_ref();
        let __tuiscotti_screen = $screen;
        let __tuiscotti_policy: &$crate::assert::Policy = $policy;
        let __tuiscotti_location = $crate::assert::Location {
            file: file!(),
            line: line!(),
        };
        match __tuiscotti_policy {
            $crate::assert::Policy::Evolving => {
                let __tuiscotti_dir = $crate::assert::default_snapshot_dir();
                $crate::assert_screenshot_in!(
                    __tuiscotti_name,
                    __tuiscotti_screen,
                    __tuiscotti_location,
                    __tuiscotti_dir,
                    $crate::assert::evidence_dir()
                );
            }
            $crate::assert::Policy::EvolvingIn {
                snapshots,
                evidence,
            } => {
                $crate::assert_screenshot_in!(
                    __tuiscotti_name,
                    __tuiscotti_screen,
                    __tuiscotti_location,
                    snapshots.clone(),
                    evidence.clone()
                );
            }
            $crate::assert::Policy::Frozen { root } => {
                $crate::assert::assert_frozen_screenshot(
                    root,
                    __tuiscotti_name,
                    __tuiscotti_screen,
                );
            }
        }
    }};
}

/// Evolving screenshot body shared by both [`crate::assert_screenshot!`] policy arms.
///
/// Macro-internal (`#[doc(hidden)]`): runs the one-sample flow — prepare
/// (render + evidence BEFORE any failure), assert canonical, assert PNG by
/// decoded pixels, run the lenient compound gate — with both Insta assertions
/// expanding at the original caller.
#[doc(hidden)]
#[macro_export]
macro_rules! assert_screenshot_in {
    ($name:expr, $screen:expr, $location:expr, $snapshots:expr, $evidence:expr) => {{
        let __tuiscotti_snapshots = $snapshots;
        let __tuiscotti_evidence = $evidence;
        let __tuiscotti_prepared =
            $crate::assert::prepare_screenshot($name, $screen, &__tuiscotti_evidence);
        let __tuiscotti_settings = $crate::assert::snapshot_settings(
            &__tuiscotti_snapshots,
            $location,
            &__tuiscotti_prepared.generation,
        );
        let __tuiscotti_snap_name = $name.to_string();
        let __tuiscotti_canonical = __tuiscotti_prepared.canonical;
        __tuiscotti_settings.bind(|| {
            $crate::insta::assert_snapshot!(
                __tuiscotti_snap_name,
                __tuiscotti_canonical,
                "canonical screen"
            );
        });
        let __tuiscotti_png_base = $crate::assert::png_snapshot_base($name);
        let __tuiscotti_png_name = format!("{}.png", __tuiscotti_png_base);
        let mut __tuiscotti_png_settings = $crate::assert::snapshot_settings(
            &__tuiscotti_snapshots,
            $location,
            &__tuiscotti_prepared.generation,
        );
        __tuiscotti_png_settings
            .set_comparator(Box::new($crate::assert::screenshot_png_comparator()));
        let __tuiscotti_png = __tuiscotti_prepared.png;
        __tuiscotti_png_settings.bind(|| {
            $crate::insta::assert_binary_snapshot!(
                __tuiscotti_png_name.as_str(),
                __tuiscotti_png,
                "screenshot png"
            );
        });
        if let Err(__tuiscotti_err) = $crate::assert::check_consistent_lenient(
            &__tuiscotti_snapshots,
            $name,
            &__tuiscotti_png_base,
        ) {
            panic!(
                "tuisnap assert_screenshot!({:?}): {}",
                $name, __tuiscotti_err
            );
        }
    }};
}
