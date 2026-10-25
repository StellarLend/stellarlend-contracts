//! Centralised error re-exports.
//!
//! `lib.rs` references `errors::GovernanceError` throughout its entrypoint
//! signatures.  The canonical definition lives in `governance.rs`; this module
//! re-exports it so the short path resolves.

pub use crate::governance::GovernanceError;

#[cfg(test)]
mod tests {
    //! Regression tests pinning the `crate::errors::GovernanceError` re-export
    //! (issue #2020).
    //!
    //! `lib.rs` used to qualify its governance entrypoint signatures with
    //! `errors::GovernanceError` while the type only existed in
    //! `crate::governance`.  The re-export above keeps the short path resolving.
    //! These tests make that guarantee explicit: the two paths must name the
    //! *same* type, so a future refactor cannot silently reintroduce the
    //! `cannot find type GovernanceError in module 'errors'` break.

    use super::GovernanceError as ReExported;
    use crate::governance::GovernanceError as Canonical;

    /// Compiles only if the short path and the canonical path are the same
    /// type — the core guarantee of this module.
    fn same_type(error: ReExported) -> Canonical {
        error
    }

    #[test]
    fn re_export_names_the_canonical_governance_error() {
        // A representative variant constructed through the re-exported path.
        let via_short_path = ReExported::Unauthorized;
        let canonical: Canonical = same_type(via_short_path);
        assert_eq!(canonical, Canonical::Unauthorized);
    }

    #[test]
    fn canonical_variants_are_reachable_through_the_re_export() {
        // Spot-check a few variants to guard the whole enum being re-exported
        // rather than a subset.
        assert_eq!(ReExported::Unauthorized, Canonical::Unauthorized);
        assert_eq!(
            ReExported::ProposalNotFound,
            Canonical::ProposalNotFound
        );
        assert_eq!(ReExported::QuorumNotMet, Canonical::QuorumNotMet);
    }
}
