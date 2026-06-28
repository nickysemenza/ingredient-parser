//! Shared stage-pipeline infrastructure for normalize, recognize, and refine.
//!
//! [`define_stage_pipeline!`](crate::define_stage_pipeline) generates an Id enum,
//! Entry struct, and ordered TABLE const from a single row list.

/// Declare a lazy-compiled regex. Pass a string literal pattern, or a block
/// expression that evaluates to a `regex::Regex` (for patterns built with `format!`).
#[macro_export]
macro_rules! lazy_regex {
    ($name:ident, $pat:literal) => {
        static $name: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            #[allow(clippy::expect_used)]
            regex::Regex::new($pat).expect(concat!("invalid regex: ", $pat))
        });
    };
    ($name:ident, { $($body:tt)* }) => {
        static $name: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            #[allow(clippy::expect_used)]
            { $($body)* }
        });
    };
}

/// From one row list, generate Id + as_str + Entry + TABLE. Each row is
/// `(Variant, "trace_label", run_fn)`. Set `trace: none` or
/// `trace: pub(crate) TRACE_NAMES` to optionally emit a label slice for tracing.
#[macro_export]
macro_rules! define_stage_pipeline {
    (
        $(#[$enum_meta:meta])*
        $id_vis:vis enum $id:ident,
        $entry_vis:vis struct $entry:ident,
        $table_vis:vis const $table:ident: &[$entry_ty:ident],
        type $run_ty:ident = $run_ty_sig:ty,
        trace: none,
        $( ( $variant:ident, $label:literal, $run:expr ) ),+ $(,)?
    ) => {
        $crate::define_stage_pipeline! {
            @core
            $(#[$enum_meta])*
            $id_vis enum $id,
            $entry_vis struct $entry,
            $table_vis const $table: &[$entry_ty],
            type $run_ty = $run_ty_sig,
            $( ( $variant, $label, $run ) ),+
        }
    };
    (
        $(#[$enum_meta:meta])*
        $id_vis:vis enum $id:ident,
        $entry_vis:vis struct $entry:ident,
        $table_vis:vis const $table:ident: &[$entry_ty:ident],
        type $run_ty:ident = $run_ty_sig:ty,
        trace: $trace_vis:vis $trace_names:ident,
        $( ( $variant:ident, $label:literal, $run:expr ) ),+ $(,)?
    ) => {
        $crate::define_stage_pipeline! {
            @core
            $(#[$enum_meta])*
            $id_vis enum $id,
            $entry_vis struct $entry,
            $table_vis const $table: &[$entry_ty],
            type $run_ty = $run_ty_sig,
            $( ( $variant, $label, $run ) ),+
        }
        $trace_vis const $trace_names: &[&str] = &[
            $( $label ),+
        ];
    };
    (
        @core
        $(#[$enum_meta:meta])*
        $id_vis:vis enum $id:ident,
        $entry_vis:vis struct $entry:ident,
        $table_vis:vis const $table:ident: &[$entry_ty:ident],
        type $run_ty:ident = $run_ty_sig:ty,
        $( ( $variant:ident, $label:literal, $run:expr ) ),+ $(,)?
    ) => {
        $(#[$enum_meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $id_vis enum $id {
            $( $variant ),+
        }

        impl $id {
            pub(crate) const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $label ),+
                }
            }
        }

        #[derive(Clone, Copy)]
        $entry_vis struct $entry {
            id: $id,
            run: $run_ty,
        }

        impl $entry {
            const fn new(id: $id, run: $run_ty) -> Self {
                Self { id, run }
            }

            pub(crate) const fn id(self) -> $id {
                self.id
            }
        }

        $table_vis const $table: &[$entry_ty] = &[
            $( $entry::new($id::$variant, $run) ),+
        ];
    };
}

/// Assert that a stage pipeline table has unique ids and unique, non-empty labels.
#[macro_export]
macro_rules! assert_stage_pipeline {
    ($table:expr) => {{
        use std::collections::HashSet;
        let ids: HashSet<_> = $table.iter().map(|entry| entry.id()).collect();
        assert_eq!(ids.len(), $table.len(), "duplicate stage ids");
        let labels: HashSet<_> = $table.iter().map(|entry| entry.id().as_str()).collect();
        assert_eq!(labels.len(), $table.len(), "duplicate stage labels");
        for entry in $table {
            assert!(!entry.id().as_str().is_empty(), "empty stage label");
        }
    }};
}
