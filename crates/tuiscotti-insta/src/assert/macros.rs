//! Facade macros plus the caller [`Location`](super::Location) they capture.
//!
//! Both macros expand the Insta assertions AT THE CALLER and drive them from
//! ONE resolved [`SnapshotIdentity`](super::SnapshotIdentity) (caller
//! manifest dir + caller file + snapshot path + active suffix), so the Insta
//! settings, the consistency gate, and the evidence manifest can never
//! disagree about which files they mean.

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
        // `env!` expands at the caller: the CALLER's manifest dir, which is
        // what Insta joins the caller-relative snapshot path against.
        let __tuiscotti_manifest: &str = env!("CARGO_MANIFEST_DIR");
        match __tuiscotti_policy {
            $crate::assert::Policy::Evolving => {
                let __tuiscotti_identity = $crate::assert::resolve_snapshot_identity(
                    __tuiscotti_manifest,
                    __tuiscotti_location,
                    __tuiscotti_name,
                    $crate::assert::snapshot_dir_override().as_deref(),
                );
                let (__tuiscotti_canonical, __tuiscotti_generation) =
                    $crate::assert::prepare_snapshot(__tuiscotti_screen);
                let __tuiscotti_settings = $crate::assert::snapshot_settings(
                    &__tuiscotti_identity.dir,
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
                let __tuiscotti_identity = $crate::assert::resolve_snapshot_identity_in(
                    __tuiscotti_manifest,
                    __tuiscotti_location,
                    snapshots,
                    __tuiscotti_name,
                );
                let (__tuiscotti_canonical, __tuiscotti_generation) =
                    $crate::assert::prepare_snapshot(__tuiscotti_screen);
                let __tuiscotti_settings = $crate::assert::snapshot_settings(
                    &__tuiscotti_identity.dir,
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
/// The full candidate bundle (canonical + tagged image + ANSI/TXT/HTML +
/// manifest) is published under [`EVIDENCE_DIR_ENV`](super::EVIDENCE_DIR_ENV)
/// BEFORE any failure. The PNG snapshot is named `<name>-img` (plus the
/// active Insta suffix, if any). Both Insta assertions run even when the
/// first fails, so one run produces BOTH pendings; failures aggregate into a
/// single panic AFTER the strict compound gate ran. Both Insta assertions
/// expand AT THE CALLER. Each argument is evaluated exactly once.
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
        let __tuiscotti_manifest: &str = env!("CARGO_MANIFEST_DIR");
        let __tuiscotti_package: &str = env!("CARGO_PKG_NAME");
        match __tuiscotti_policy {
            $crate::assert::Policy::Evolving => {
                let __tuiscotti_identity = $crate::assert::resolve_snapshot_identity(
                    __tuiscotti_manifest,
                    __tuiscotti_location,
                    __tuiscotti_name,
                    $crate::assert::snapshot_dir_override().as_deref(),
                );
                $crate::assert_screenshot_in!(
                    __tuiscotti_name,
                    __tuiscotti_screen,
                    __tuiscotti_location,
                    __tuiscotti_identity,
                    __tuiscotti_package,
                    $crate::assert::evidence_dir()
                );
            }
            $crate::assert::Policy::EvolvingIn {
                snapshots,
                evidence,
            } => {
                let __tuiscotti_identity = $crate::assert::resolve_snapshot_identity_in(
                    __tuiscotti_manifest,
                    __tuiscotti_location,
                    snapshots,
                    __tuiscotti_name,
                );
                $crate::assert_screenshot_in!(
                    __tuiscotti_name,
                    __tuiscotti_screen,
                    __tuiscotti_location,
                    __tuiscotti_identity,
                    __tuiscotti_package,
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
/// Macro-internal (`#[doc(hidden)]`): runs the one-sample flow — publish the
/// full candidate bundle BEFORE any failure, run BOTH Insta assertions (a
/// canonical failure never suppresses the PNG pending), run the STRICT
/// compound gate over the resolved identity, then surface one aggregated
/// failure — with both Insta assertions expanding at the original caller.
#[doc(hidden)]
#[macro_export]
macro_rules! assert_screenshot_in {
    ($name:expr, $screen:expr, $location:expr, $identity:expr, $package:expr, $evidence:expr) => {{
        let __tuiscotti_identity = $identity;
        let __tuiscotti_evidence = $evidence;
        let __tuiscotti_prepared = $crate::assert::prepare_screenshot(
            $name,
            $screen,
            &__tuiscotti_evidence,
            $package,
            &__tuiscotti_identity,
        );
        let __tuiscotti_settings = $crate::assert::snapshot_settings(
            &__tuiscotti_identity.dir,
            $location,
            &__tuiscotti_prepared.binding,
        );
        let __tuiscotti_snap_name = $name.to_string();
        let __tuiscotti_canonical = __tuiscotti_prepared.canonical;
        let __tuiscotti_canonical_outcome = std::panic::catch_unwind(
            std::panic::AssertUnwindSafe(|| {
                __tuiscotti_settings.bind(|| {
                    $crate::insta::assert_snapshot!(
                        __tuiscotti_snap_name,
                        __tuiscotti_canonical,
                        "canonical screen"
                    );
                });
            }),
        );
        let __tuiscotti_png_base = $crate::assert::png_snapshot_base($name);
        let __tuiscotti_png_name = format!("{}.png", __tuiscotti_png_base);
        let mut __tuiscotti_png_settings = $crate::assert::snapshot_settings(
            &__tuiscotti_identity.dir,
            $location,
            &__tuiscotti_prepared.binding,
        );
        __tuiscotti_png_settings
            .set_comparator(Box::new($crate::assert::screenshot_png_comparator()));
        let __tuiscotti_png = __tuiscotti_prepared.png;
        let __tuiscotti_png_outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            __tuiscotti_png_settings.bind(|| {
                $crate::insta::assert_binary_snapshot!(
                    __tuiscotti_png_name.as_str(),
                    __tuiscotti_png,
                    "screenshot png"
                );
            });
        }));
        // Strict gate over the RESOLVED (suffixed, caller-relative) identity:
        // new-format samples require complete bindings; missing or partial
        // bindings fail here instead of passing half-blind.
        let __tuiscotti_gate = $crate::assert::check_consistent(
            &__tuiscotti_identity.dir,
            &__tuiscotti_identity.canonical,
            &__tuiscotti_identity.png_base,
        );
        if let Some(__tuiscotti_failure) = $crate::assert::aggregate_compound_result(
            $name,
            &__tuiscotti_canonical_outcome,
            &__tuiscotti_png_outcome,
            &__tuiscotti_gate,
            &__tuiscotti_prepared.bundle_dir,
        ) {
            panic!("{__tuiscotti_failure}");
        }
    }};
}
