// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Slab layout vocabulary for CanonArrow columnar egress — column kinds, column
//! specs, and 64-byte slab-boundary padding.
//!
//! All sizes are [`usize`] and every arithmetic step is checked: [`pad_to_slab`]
//! is total (never panics) and saturates to [`usize::MAX`] for lengths that
//! cannot be rounded up to a slab boundary without overflow.

/// Slab alignment in bytes — one cache line, matching the wire header alignment.
pub const SLAB_ALIGNMENT: usize = 64;

/// Physical storage class of a column within a slab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnKind {
    /// Fixed-width one-byte elements (booleans, small enums, tags).
    U8,
    /// Fixed-width four-byte elements (interned ids, offsets, lengths).
    U32,
    /// Variable-width byte region (UTF-8 text, opaque blobs).
    Bytes,
}

/// Static description of one column: its physical [`ColumnKind`] and its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnSpec {
    /// Physical storage class of the column.
    pub kind: ColumnKind,
    /// Static column name (wire-facing, never heap-allocated).
    pub name: &'static str,
}

/// Layout of a slab: the ordered column specifications it carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlabLayout<'a> {
    /// Ordered column specifications for the slab.
    pub columns: &'a [ColumnSpec],
}

/// Round `len` up to the next multiple of [`SLAB_ALIGNMENT`] (64).
///
/// Overflow policy: for `len` values whose rounded-up size cannot be
/// represented in [`usize`], this returns [`usize::MAX`] (saturating) rather
/// than panicking. Callers treat [`usize::MAX`] as "no slab fits" — it is not
/// a valid slab size, since every valid slab size is a positive multiple of
/// [`SLAB_ALIGNMENT`].
pub fn pad_to_slab(len: usize) -> usize {
    let Some(rem) = len.checked_rem(SLAB_ALIGNMENT) else {
        // Division by zero is impossible: SLAB_ALIGNMENT is a nonzero constant.
        return usize::MAX;
    };
    if rem == 0 {
        return len;
    }
    match len.checked_sub(rem) {
        Some(base) => match base.checked_add(SLAB_ALIGNMENT) {
            Some(padded) => padded,
            // `base + 64` overflows only within a hair of usize::MAX; saturate.
            None => usize::MAX,
        },
        None => usize::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::{SLAB_ALIGNMENT, pad_to_slab};

    #[test]
    fn zero_pads_to_zero() {
        assert_eq!(pad_to_slab(0), 0);
    }

    #[test]
    fn one_pads_to_one_slab() {
        assert_eq!(pad_to_slab(1), 64);
    }

    #[test]
    fn exact_slab_is_unchanged() {
        assert_eq!(pad_to_slab(64), 64);
    }

    #[test]
    fn just_past_slab_pads_to_next_boundary() {
        assert_eq!(pad_to_slab(65), 128);
    }

    #[test]
    fn huge_len_saturates_without_panicking() {
        let huge = usize::MAX;
        assert_eq!(pad_to_slab(huge), usize::MAX);
        // Near-boundary case: base + 64 would overflow but len itself fits.
        let near = usize::MAX - 32;
        assert_eq!(pad_to_slab(near), usize::MAX);
        // Every finite result is a positive multiple of the alignment.
        for len in [0usize, 1, 63, 64, 65, 127, 128, 1 << 20, usize::MAX - 1] {
            let padded = pad_to_slab(len);
            if padded != usize::MAX {
                assert_eq!(padded % SLAB_ALIGNMENT, 0);
                assert!(padded >= len);
            }
        }
    }
}
