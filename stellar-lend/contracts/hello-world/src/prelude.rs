// Common imports for WASM compilation
pub use core::option::Option::{self, Some, None};
pub use core::result::Result::{self, Ok, Err};
pub use core::convert::{Into, From, TryFrom, TryInto};
pub use core::ops::Drop;
pub use core::clone::Clone;
pub use core::iter::{Iterator, ExactSizeIterator};
pub use core::default::Default;
pub use core::cmp::{PartialEq, Eq, PartialOrd, Ord};
pub use core::fmt::Debug;
pub use core::marker::Copy;

// Re-export derive macro for struct definitions
pub use core::prelude::rust_2024::derive;

// Re-export panic macro
pub use core::panic;